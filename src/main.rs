use alloy::{
    eips::BlockId,
    network::Ethereum,
    providers::{ProviderBuilder, RootProvider},
    sol,
    sol_types::{SolCall, SolValue},
    transports::http::{reqwest::Url, Client, Http},
};
use anyhow::{anyhow, Result};
use clap::Parser;
use erc20::{get_erc20_info, ERC20};
use revm::{
    db::{AlloyDB, CacheDB},
    primitives::{
        address, keccak256, AccountInfo, Address, Bytes, ExecutionResult, Output, TxKind, U256,
    },
    Evm,
};
use uniswapv2::{get_weth_pair, UniV2Pair};

mod erc20;
mod uniswapv2;

// TODO:
// - Take RPC_URL from env variables. Currently using local Reth node.
// - Improve swap error handling
// - Add more options to the CLI (more logs, more details about token etc)

const WETH: Address = address!("c02aaa39b223fe8d0a0e5c4f27ead9083c756cc2");
type AlloyCacheDB = CacheDB<AlloyDB<Http<Client>, Ethereum, RootProvider<Http<Client>>>>;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Cli {
    /// ERC20 token address to test
    token: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // You can check the value provided by positional arguments, or option arguments
    let token: Address = cli.token.parse().expect("Address is not valid");

    let rpc_url = Url::parse("http://192.168.0.212:8545").unwrap();
    let client = ProviderBuilder::new().on_http(rpc_url);

    let token = get_erc20_info(&token, &client).await?;
    println!("Token address: {}", token.address);
    println!("Token name: {}", token.name);
    println!("Token symbol: {}", token.symbol);
    println!("Token decimals: {}", token.decimals);
    println!("");

    let univ2_pair = get_weth_pair(&token.address, &client).await?;
    println!("WETH/{} UniV2 pair: {}", token.symbol, univ2_pair.address);

    test_token(univ2_pair, token).await?;

    Ok(())
}

// #[tokio::main]
async fn test_token(pair: UniV2Pair, token: ERC20) -> Result<()> {
    // Local Reth node
    let rpc_url = Url::parse("http://192.168.0.212:8545").unwrap();
    let client = ProviderBuilder::new().on_http(rpc_url);

    let mut cache_db = get_cache_db(client)?;

    let acc = address!("EAa1E618F9Bf501BC12680630605e1765dE6d916");
    let weth_balance_slot = U256::from(3);

    // 10 WETH
    let ten_eth = U256::from(10_u128.pow(18));
    let weth_user_balance_slot = keccak256((acc, weth_balance_slot).abi_encode());

    cache_db
        .insert_account_storage(WETH, weth_user_balance_slot.into(), ten_eth)
        .expect("Failed to insert account storage");

    cache_db.insert_account_info(
        acc,
        AccountInfo {
            balance: ten_eth,
            ..Default::default()
        },
    );

    // let weth_balance_before = balance_of(WETH, acc, &mut cache_db)?;
    // println!("WETH balance before swap: {}", weth_balance_before);
    // let token_balance_before = balance_of(token.address, acc, &mut cache_db)?;
    // println!(
    //     "{} balance before swap: {}",
    //     token.symbol, token_balance_before
    // );

    let amount_in = ten_eth.div_ceil(U256::from(10));
    let (reserve0, reserve1) = get_reserves(pair.address, &mut cache_db)?;

    let weth_is_token_0 = pair.token0 == WETH;
    let (reserve_in, reserve_out) = if weth_is_token_0 {
        (reserve0, reserve1)
    } else {
        (reserve1, reserve0)
    };

    // calculate token amount out
    let amount_out = get_amount_out(amount_in, reserve_in, reserve_out, &mut cache_db).await?;

    // transfer WETH to TOKEN-WETH pair
    transfer(acc, pair.address, amount_in, WETH, &mut cache_db)?;

    // execute low-level swap without using UniswapV2 router
    swap(
        acc,
        pair.address,
        acc,
        amount_out,
        !weth_is_token_0,
        &mut cache_db,
    )?;

    transfer(acc, pair.address, amount_out, token.address, &mut cache_db)?;
    let weth_amount_out =
        get_amount_out(amount_out, reserve_out, reserve_in, &mut cache_db).await?;

    swap(
        acc,
        pair.address,
        acc,
        weth_amount_out,
        weth_is_token_0,
        &mut cache_db,
    )?;

    // let weth_balance_after = balance_of(WETH, acc, &mut cache_db)?;
    // println!("WETH balance after swap: {}", weth_balance_after);
    // let token_balance_after = balance_of(token.address, acc, &mut cache_db)?;
    // println!(
    //     "{} balance after swap: {}",
    //     token.symbol, token_balance_after
    // );

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
    target: Address,
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
        target,
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
