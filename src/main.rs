use std::str::FromStr;

use alloy::{
    eips::BlockId,
    network::Ethereum,
    primitives::{address, keccak256, Address, TxKind, U256},
    providers::{Provider, ProviderBuilder, RootProvider},
    sol,
    sol_types::{SolCall, SolValue},
    transports::http::{reqwest::Url, Client, Http},
};
use anyhow::{anyhow, Result};
use clap::Parser;
use erc20::{get_erc20_info, ERC20};

use revm::{
    db::{AlloyDB, CacheDB},
    primitives::{AccountInfo, Bytes, ExecutionResult, Output},
    Evm,
};
use revm_actions::{balance_of, get_univ2_reserves, univ2_swap};
use uniswapv2::{get_weth_pair, UniV2Pair};
use uniswapv3::test_swap;

mod erc20;
mod revm_actions;
mod uniswapv2;
mod uniswapv3;

// TODO:
// - Take RPC_URL from env variables. Currently using local Reth node.
// - Improve swap error handling
// - Add more options to the CLI (more logs, more details about token, use specific sender)

const WETH: Address = address!("c02aaa39b223fe8d0a0e5c4f27ead9083c756cc2");
const DEFAULT_ACC: Address = address!("e4A6aD6E1B86AB8f2d2f571717592De46bFaF614");

type AlloyCacheDB = CacheDB<AlloyDB<Http<Client>, Ethereum, RootProvider<Http<Client>>>>;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
#[command(next_line_help = true)] // This ensures help text appears on the next line
struct Cli {
    /// ERC20 token address to test
    token: String,
    /// Enable full logging
    #[arg(short, long, default_value_t = false)]
    logs: bool,

    /// Address from which the test will be done
    #[arg(short, long)]
    sender: Option<String>,

    /// The RPC endpoint.
    /// If no ETH_RPC_URL is set or no rpc_url is not passed, by default
    /// Flashbots RPC URL will be used
    #[arg(
        short,
        long,
        env = "ETH_RPC_URL",
        default_value = "https://rpc.flashbots.net/fast"
    )]
    rpc_url: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // You can check the value provided by positional arguments, or option arguments
    let token: Address = cli.token.parse().expect("Address is not valid");
    let rpc_url_string: String = cli.rpc_url.parse().expect("Error getting RPC URL");
    let logs_enabled: bool = cli.logs;
    let sender = if let Some(_sender) = cli.sender {
        Address::from_str(&_sender).map_err(|e| anyhow!("Error parsing sender's address: {e}"))?
    } else {
        DEFAULT_ACC
    };

    let rpc_url = Url::parse(&rpc_url_string).map_err(|e| anyhow!("Error parsing RPC URL: {e}"))?;
    let client = ProviderBuilder::new().on_http(rpc_url);
    let chain_id = client
        .get_chain_id()
        .await
        .map_err(|e| anyhow!("Unable to fetch Chain ID: {e}"))?;
    if chain_id != 1 {
        anyhow::bail!(
            "Only mainnet is supported, the provided RPC URL is for chain {}",
            chain_id
        );
    }

    test_swap(&mut get_cache_db(client)?)?;

    Ok(())

    // let token = get_erc20_info(&token, &client).await?;
    // if logs_enabled {
    //     println!("Token address: {}", token.address);
    //     println!("Token name: {}", token.name);
    //     println!("Token symbol: {}", token.symbol);
    //     println!("Token decimals: {}", token.decimals);
    //     println!("");
    // }

    // let univ2_pair = get_weth_pair(&token.address, &client).await?;

    // if logs_enabled {
    //     println!("WETH/{} UniV2 pair: {}", token.symbol, univ2_pair.address);
    // }

    // test_token(univ2_pair, token, sender, client, logs_enabled)
    //     .await
    //     .map_err(|e| anyhow!("Test failed, token is most likely a honeypot: {e}"))?;

    // Ok(())
}

// #[tokio::main]
async fn test_token(
    pair: UniV2Pair,
    token: ERC20,
    sender: Address,
    client: RootProvider<Http<Client>>,
    logs_enabled: bool,
) -> Result<()> {
    let mut cache_db = get_cache_db(client)?;

    // 1. Add WETH to account
    let weth_balance_slot = U256::from(3);
    let one_eth = U256::from(10_u128.pow(18));
    let weth_user_balance_slot = keccak256((sender, weth_balance_slot).abi_encode());

    cache_db
        .insert_account_storage(WETH, weth_user_balance_slot.into(), one_eth)
        .expect("Failed to insert account storage");

    cache_db.insert_account_info(
        sender,
        AccountInfo {
            balance: one_eth,
            ..Default::default()
        },
    );

    if logs_enabled {
        let weth_balance_before = balance_of(WETH, sender, sender, &mut cache_db)?;
        println!("WETH balance before swap: {}", weth_balance_before);
        let token_balance_before = balance_of(token.address, sender, sender, &mut cache_db)?;
        println!(
            "{} balance before swap: {}",
            token.symbol, token_balance_before
        );
    }

    let amount_in = one_eth.div_ceil(U256::from(10));
    let reserves = get_univ2_reserves(pair.address, sender, &mut cache_db)?;

    // 2. Swap WETH for Token
    let amount_out = univ2_swap(sender, &pair, WETH, amount_in, reserves, &mut cache_db)?;

    // 3. Swap Token for WETH
    //    this is what shows if the token is a honeypot or not.
    let reserves = get_univ2_reserves(pair.address, sender, &mut cache_db)?;
    univ2_swap(
        sender,
        &pair,
        token.address,
        amount_out,
        reserves,
        &mut cache_db,
    )?;

    if logs_enabled {
        let weth_balance_after = balance_of(WETH, sender, sender, &mut cache_db)?;
        println!("WETH balance after swap: {}", weth_balance_after);
        let token_balance_after = balance_of(token.address, sender, sender, &mut cache_db)?;
        println!(
            "{} balance after swap: {}",
            token.symbol, token_balance_after
        );
    }

    println!("\n Successful Swap \n");

    Ok(())
}

fn get_cache_db(client: RootProvider<Http<Client>>) -> Result<AlloyCacheDB> {
    let db = AlloyDB::new(client, BlockId::latest()).expect("Failed to create Revm Alloy DB");
    Ok(CacheDB::new(db))
}
