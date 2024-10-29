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
use revm_actions::univ2_swap;
use uniswapv2::{get_weth_pair, UniV2Pair};

mod erc20;
mod revm_actions;
mod uniswapv2;

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

    let token = get_erc20_info(&token, &client).await?;
    if logs_enabled {
        println!("Token address: {}", token.address);
        println!("Token name: {}", token.name);
        println!("Token symbol: {}", token.symbol);
        println!("Token decimals: {}", token.decimals);
        println!("");
    }

    let univ2_pair = get_weth_pair(&token.address, &client).await?;

    if logs_enabled {
        println!("WETH/{} UniV2 pair: {}", token.symbol, univ2_pair.address);
    }

    test_token(univ2_pair, token, sender, client, logs_enabled)
        .await
        .map_err(|e| anyhow!("Test failed, token is most likely a honeypot: {e}"))?;

    Ok(())
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
        let weth_balance_before = balance_of(WETH, sender, &mut cache_db)?;
        println!("WETH balance before swap: {}", weth_balance_before);
        let token_balance_before = balance_of(token.address, sender, &mut cache_db)?;
        println!(
            "{} balance before swap: {}",
            token.symbol, token_balance_before
        );
    }

    let amount_in = one_eth.div_ceil(U256::from(10));
    let reserves = get_reserves(pair.address, &mut cache_db)?;

    // 2. Swap WETH for Token
    let amount_out = univ2_swap(sender, &pair, WETH, amount_in, reserves, &mut cache_db)?;

    // 3. Swap Token for WETH
    //    this is what shows if the token is a honeypot or not.
    let reserves = get_reserves(pair.address, &mut cache_db)?;
    univ2_swap(
        sender,
        &pair,
        token.address,
        amount_out,
        reserves,
        &mut cache_db,
    )?;

    if logs_enabled {
        let weth_balance_after = balance_of(WETH, sender, &mut cache_db)?;
        println!("WETH balance after swap: {}", weth_balance_after);
        let token_balance_after = balance_of(token.address, sender, &mut cache_db)?;
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

fn balance_of(token: Address, address: Address, alloy_db: &mut AlloyCacheDB) -> Result<U256> {
    sol! {
        function balanceOf(address account) public returns (uint256);
    }

    let encoded = balanceOfCall { account: address }.abi_encode();

    let mut evm = Evm::builder()
        .with_db(alloy_db)
        .modify_tx_env(|tx| {
            tx.caller = address!("0000000000000000000000000000000000000001");
            tx.transact_to = TxKind::Call(token);
            tx.data = encoded.into();
            tx.value = U256::from(0);
        })
        .build();

    let ref_tx = evm.transact().unwrap();
    let result = ref_tx.result;

    let value = match result {
        ExecutionResult::Success {
            output: Output::Call(value),
            ..
        } => value,
        result => return Err(anyhow!("'balanceOf' execution failed: {result:?}")),
    };

    let balance = <U256>::abi_decode(&value, false)?;

    Ok(balance)
}

fn transfer(
    from: Address,
    to: Address,
    amount: U256,
    token: Address,
    cache_db: &mut AlloyCacheDB,
) -> Result<()> {
    sol! {
        function transfer(address to, uint amount) external returns (bool);
    }

    let encoded = transferCall { to, amount }.abi_encode();

    let mut evm = Evm::builder()
        .with_db(cache_db)
        .modify_tx_env(|tx| {
            tx.caller = from;
            tx.transact_to = TxKind::Call(token);
            tx.data = encoded.into();
            tx.value = U256::from(0);
        })
        .build();

    let ref_tx = evm.transact_commit().unwrap();
    let success: bool = match ref_tx {
        ExecutionResult::Success {
            output: Output::Call(value),
            ..
        } => {
            // This is because some ERC20 tokens do not return true/false
            if value.len() > 0 {
                <bool>::abi_decode(&value, false)?
            } else {
                true
            }
        }
        result => return Err(anyhow!("'transfer' execution failed: {result:?}")),
    };

    if !success {
        return Err(anyhow!("'transfer' failed"));
    }

    Ok(())
}

fn swap(
    from: Address,
    pool_address: Address,
    amount_out: U256,
    is_token0: bool,
    cache_db: &mut AlloyCacheDB,
) -> Result<()> {
    sol! {
        function swap(uint amount0Out, uint amount1Out, address target, bytes callback) external;
    }

    let amount0_out = if is_token0 { amount_out } else { U256::from(0) };
    let amount1_out = if is_token0 { U256::from(0) } else { amount_out };

    let encoded = swapCall {
        amount0Out: amount0_out,
        amount1Out: amount1_out,
        target: from,
        callback: Bytes::new(),
    }
    .abi_encode();

    let mut evm = Evm::builder()
        .with_db(cache_db)
        .modify_tx_env(|tx| {
            tx.caller = from;
            tx.transact_to = TxKind::Call(pool_address);
            tx.data = encoded.into();
            tx.value = U256::from(0);
            // tx.nonce = Some(1);
        })
        .build();

    let ref_tx = evm.transact_commit().unwrap();

    match ref_tx {
        ExecutionResult::Success { .. } => {}
        result => return Err(anyhow!("'swap' execution failed: {result:?}")),
    };

    Ok(())
}

fn get_reserves(pair_address: Address, cache_db: &mut AlloyCacheDB) -> Result<(U256, U256)> {
    sol! {
        function getReserves() external view returns (uint112 reserve0, uint112 reserve1, uint32 blockTimestampLast);
    }

    let encoded = getReservesCall {}.abi_encode();

    let mut evm = Evm::builder()
        .with_db(cache_db)
        .modify_tx_env(|tx| {
            tx.caller = address!("0000000000000000000000000000000000000000");
            tx.transact_to = TxKind::Call(pair_address);
            tx.data = encoded.into();
            tx.value = U256::from(0);
        })
        .build();

    let ref_tx = evm.transact().unwrap();
    let result = ref_tx.result;

    let value = match result {
        ExecutionResult::Success {
            output: Output::Call(value),
            ..
        } => value,
        result => return Err(anyhow!("'getReserves' execution failed: {result:?}")),
    };

    let (reserve0, reserve1, _) = <(U256, U256, u32)>::abi_decode(&value, false)?;

    Ok((reserve0, reserve1))
}

async fn get_amount_out(
    amount_in: U256,
    reserve_in: U256,
    reserve_out: U256,
    cache_db: &mut AlloyCacheDB,
) -> Result<U256> {
    let uniswap_v2_router = address!("7a250d5630b4cf539739df2c5dacb4c659f2488d");
    sol! {
        function getAmountOut(uint amountIn, uint reserveIn, uint reserveOut) external pure returns (uint amountOut);
    }

    let encoded = getAmountOutCall {
        amountIn: amount_in,
        reserveIn: reserve_in,
        reserveOut: reserve_out,
    }
    .abi_encode();

    let mut evm = Evm::builder()
        .with_db(cache_db)
        .modify_tx_env(|tx| {
            tx.caller = address!("0000000000000000000000000000000000000000");
            tx.transact_to = TxKind::Call(uniswap_v2_router);
            tx.data = encoded.into();
            tx.value = U256::from(0);
        })
        .build();

    let ref_tx = evm.transact().unwrap();
    let result = ref_tx.result;

    let value = match result {
        ExecutionResult::Success {
            output: Output::Call(value),
            ..
        } => value,
        result => return Err(anyhow!("'getAmountOut' execution failed: {result:?}")),
    };

    let amount_out = <U256>::abi_decode(&value, false)?;

    Ok(amount_out)
}
