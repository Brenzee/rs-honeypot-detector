use alloy::{
    primitives::{keccak256, Address},
    providers::{Provider, RootProvider},
    rpc::types::TransactionRequest,
    sol,
    sol_types::{SolCall, SolValue},
    transports::http::{Client, Http},
};
// use anyhow::Result;
use revm::{
    primitives::{address, AccountInfo, Bytes, ExecutionResult, Output, TxKind, U256},
    Evm,
};

use crate::{
    cli::CliConfig,
    erc20::ERC20,
    error::{HPError, Result},
    test_swap::TestSwap,
    AlloyCacheDB,
};

const UNIV2_FACTORY: Address = address!("5C69bEe701ef814a2B6a3EDD4B1652CB9cc5aA6f");
const UNIV2_ROUTER: Address = address!("7a250d5630b4cf539739df2c5dacb4c659f2488d");
const WETH: Address = address!("c02aaa39b223fe8d0a0e5c4f27ead9083c756cc2");

sol! {
    function balanceOf(address account) public returns (uint256);
    function transfer(address to, uint amount) external returns (bool);
    function getReserves() external view returns (uint112 reserve0, uint112 reserve1, uint32 blockTimestampLast);
    function getAmountOut(uint amountIn, uint reserveIn, uint reserveOut) external pure returns (uint amountOut);
    function swap(uint amount0Out, uint amount1Out, address target, bytes callback) external;
}

#[derive(Debug)]
pub struct UniV2Pair {
    pub address: Address,
    pub token0: Address,
    pub token1: Address,
}

pub struct UniswapV2;

impl UniswapV2 {
    pub fn new() -> Self {
        Self
    }
}

impl TestSwap for UniswapV2 {
    async fn test_swap(&self, config: &CliConfig, db: &mut AlloyCacheDB) -> Result<()> {
        let pair = get_pair(&config.token.address, &WETH, &config.client).await?;

        // 1. Add WETH to account
        let weth_balance_slot = U256::from(3);
        let one_eth = U256::from(10_u128.pow(18));
        let weth_user_balance_slot = keccak256((config.sender, weth_balance_slot).abi_encode());

        db.insert_account_storage(WETH, weth_user_balance_slot.into(), one_eth)
            .expect("Failed to insert account storage");

        db.insert_account_info(
            config.sender,
            AccountInfo {
                balance: one_eth,
                ..Default::default()
            },
        );

        if config.logs {
            let from_token_balance_before: U256 =
                config
                    .from_token
                    .balance_of(config.sender, config.sender, db)?;
            let token_balance_before: U256 =
                config.token.balance_of(config.sender, config.sender, db)?;

            println!(
                "{} balance before swap: {}",
                config.from_token.symbol, from_token_balance_before
            );
            println!(
                "{} balance before swap: {}",
                config.token.symbol, token_balance_before
            );
        }

        let amount_in = one_eth.div_ceil(U256::from(10));
        let reserves = get_univ2_reserves(pair.address, config.sender, db)?;

        // 2. Swap WETH for Token
        let amount_out = univ2_swap(
            config.sender,
            &pair,
            config.from_token.clone(),
            amount_in,
            reserves,
            db,
        )?;

        // 3. Swap Token for WETH
        //    this is what shows if the token is a honeypot or not.
        let reserves = get_univ2_reserves(pair.address, config.sender, db)?;
        univ2_swap(
            config.sender,
            &pair,
            config.token.clone(),
            amount_out,
            reserves,
            db,
        )?;

        if config.logs {
            let from_token_balance_after: U256 =
                config
                    .from_token
                    .balance_of(config.sender, config.sender, db)?;
            let token_balance_after: U256 =
                config.token.balance_of(config.sender, config.sender, db)?;

            println!(
                "{} balance after swap: {}",
                config.from_token.symbol, from_token_balance_after
            );
            println!(
                "{} balance after swap: {}",
                config.token.symbol, token_balance_after
            );
        }

        Ok(())
    }
}

pub async fn get_pair(
    token0: &Address,
    token1: &Address,
    client: &RootProvider<Http<Client>>,
) -> Result<UniV2Pair> {
    sol! {
      function getPair(address,address) public view returns (address);
    }

    let pair_calldata = getPairCall {
        _0: *token0,
        _1: *token1,
    }
    .abi_encode();

    let pair = client
        .call(&TransactionRequest {
            to: Some(TxKind::Call(UNIV2_FACTORY)),
            input: pair_calldata.into(),
            ..Default::default()
        })
        .await
        .map_err(HPError::error)?;

    let pair_res = getPairCall::abi_decode_returns(&pair, true)
        .map_err(HPError::error)?
        ._0;

    if pair_res == Address::ZERO {
        return Err(HPError::new(
            "Pair does not exist on Uniswap V2".to_string(),
            None,
        ));
    }

    let (token0, token1) = if *token0 < *token1 {
        (*token0, *token1)
    } else {
        (*token1, *token0)
    };

    Ok(UniV2Pair {
        address: pair_res,
        token0,
        token1,
    })
}

pub fn get_univ2_reserves(
    pair: Address,
    sender: Address,
    alloy_db: &mut AlloyCacheDB,
) -> Result<(U256, U256)> {
    let calldata = getReservesCall {}.abi_encode();

    let mut evm = Evm::builder()
        .with_db(alloy_db)
        .modify_tx_env(|tx| {
            // For consistency, we use the same sender for all calls
            tx.caller = sender;
            tx.transact_to = TxKind::Call(pair);
            tx.data = calldata.into();
        })
        .build();

    let tx = evm.transact().map_err(HPError::error)?;
    let result = tx.result;

    let value = match result {
        ExecutionResult::Success {
            output: Output::Call(value),
            ..
        } => value,
        result => {
            return Err(HPError::new(
                format!("'getReserves' execution failed: {result:?}"),
                None,
            ))
        }
    };

    // The output actually is u128,u128,u32, but we decode it as u256. Easier to handle.
    // Also we do not need the u32, which is the block timestamp.
    let (reserve0, reserve1, _) =
        <(U256, U256, u32)>::abi_decode(&value, false).map_err(HPError::error)?;

    Ok((reserve0, reserve1))
}

pub fn univ2_swap(
    sender: Address,
    pair: &UniV2Pair,
    token_in: ERC20,
    amount_in: U256,
    reserves: (U256, U256),
    cache_db: &mut AlloyCacheDB,
) -> Result<U256> {
    let is_token_0_in = pair.token0 == token_in.address;
    let (reserve_in, reserve_out) = if is_token_0_in {
        reserves
    } else {
        (reserves.1, reserves.0)
    };

    token_in.transfer(sender, pair.address, amount_in, cache_db)?;
    let amount_out = get_univ2_amount_out(amount_in, reserve_in, reserve_out, sender, cache_db)?;

    let amount0_out = if is_token_0_in {
        U256::from(0)
    } else {
        amount_out
    };
    let amount1_out = if is_token_0_in {
        amount_out
    } else {
        U256::from(0)
    };

    let calldata = swapCall {
        amount0Out: amount0_out,
        amount1Out: amount1_out,
        target: sender,
        callback: Bytes::new(),
    }
    .abi_encode();

    let mut evm = Evm::builder()
        .with_db(cache_db)
        .modify_tx_env(|tx| {
            tx.caller = sender;
            tx.transact_to = TxKind::Call(pair.address);
            tx.data = calldata.into();
        })
        .build();

    let tx = evm.transact_commit().map_err(HPError::error)?;

    match tx {
        ExecutionResult::Success { .. } => {}
        result => {
            return Err(HPError::new(
                format!("'swap' execution failed on Uniswap V2 pair: {result:?}"),
                None,
            ))
        }
    }

    Ok(amount_out)
}

pub fn get_univ2_amount_out(
    amount_in: U256,
    reserve_in: U256,
    reserve_out: U256,
    sender: Address,
    cache_db: &mut AlloyCacheDB,
) -> Result<U256> {
    let calldata = getAmountOutCall {
        amountIn: amount_in,
        reserveIn: reserve_in,
        reserveOut: reserve_out,
    }
    .abi_encode();

    let mut evm = Evm::builder()
        .with_db(cache_db)
        .modify_tx_env(|tx| {
            tx.caller = sender;
            tx.transact_to = TxKind::Call(UNIV2_ROUTER);
            tx.data = calldata.into();
        })
        .build();

    let tx = evm.transact().map_err(HPError::error)?;
    let result = tx.result;

    let value = match result {
        ExecutionResult::Success {
            output: Output::Call(value),
            ..
        } => value,
        result => {
            return Err(HPError::new(
                format!("'getAmountOut' execution failed: {result:?}"),
                None,
            ))
        }
    };

    let amount_out = <U256>::abi_decode(&value, false).map_err(HPError::error)?;

    Ok(amount_out)
}
