use alloy::{
    sol,
    sol_types::{SolCall, SolValue},
};
use anyhow::{anyhow, Result};
use revm::{
    primitives::{address, Address, Bytes, ExecutionResult, Output, TxKind, U256},
    Evm,
};

use crate::{uniswapv2::UniV2Pair, AlloyCacheDB, WETH};

const UNISWAP_V2_ROUTER: Address = address!("7a250d5630b4cf539739df2c5dacb4c659f2488d");

// Have a single sol! macro for all the functions we need to call
sol! {
    function balanceOf(address account) public returns (uint256);
    function transfer(address to, uint amount) external returns (bool);
    function getReserves() external view returns (uint112 reserve0, uint112 reserve1, uint32 blockTimestampLast);
    function getAmountOut(uint amountIn, uint reserveIn, uint reserveOut) external pure returns (uint amountOut);
    function swap(uint amount0Out, uint amount1Out, address target, bytes callback) external;
}

pub fn balance_of(
    token: Address,
    address: Address,
    sender: Address,
    alloy_db: &mut AlloyCacheDB,
) -> Result<U256> {
    let encoded = balanceOfCall { account: address }.abi_encode();

    let mut evm = Evm::builder()
        .with_db(alloy_db)
        .modify_tx_env(|tx| {
            // For consistency, we use the same sender for all calls
            tx.caller = sender;
            tx.transact_to = TxKind::Call(token);
            tx.data = encoded.into();
            // tx.value = U256::from(0);
        })
        .build();

    let tx = evm
        .transact()
        .map_err(|e| anyhow!("Failed to execute balanceOf call: {e}"))?;

    let result = tx.result;
    let balance = match result {
        ExecutionResult::Success {
            output: Output::Call(value),
            ..
        } => value,
        result => return Err(anyhow!("'balanceOf' execution failed: {result:?}")),
    };

    let balance = <U256>::abi_decode(&balance, false)
        .map_err(|_| anyhow!("Error decoding balanceOf result"))?;

    Ok(balance)
}

pub fn transfer(
    from: Address,
    to: Address,
    amount: U256,
    token: Address,
    alloy_db: &mut AlloyCacheDB,
) -> Result<()> {
    let calldata = transferCall { to, amount }.abi_encode();

    let mut evm = Evm::builder()
        .with_db(alloy_db)
        .modify_tx_env(|tx| {
            tx.caller = from;
            tx.transact_to = TxKind::Call(token);
            tx.data = calldata.into();
            tx.value = U256::from(0);
        })
        .build();

    let tx = evm
        .transact_commit()
        .map_err(|e| anyhow!("Failed to execute transfer call: {e}"))?;

    let res = match tx {
        ExecutionResult::Success {
            output: Output::Call(value),
            ..
        } => value,
        result => return Err(anyhow!("'transfer' execution failed: {result:?}")),
    };

    // Some tokens do not return a boolean, so we check if the result is empty
    // If it is empty, we consider the transfer successful, because if it would have failed,
    // the transaction would have failed.
    if res.len() == 0 {
        return Ok(());
    }

    let is_success =
        <bool>::abi_decode(&res, false).map_err(|_| anyhow!("Error decoding transfer result"))?;

    if !is_success {
        return Err(anyhow!("'transfer' failed"));
    }

    Ok(())
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

    let tx = evm
        .transact()
        .map_err(|e| anyhow!("Failed to execute getReserves call: {e}"))?;
    let result = tx.result;

    let value = match result {
        ExecutionResult::Success {
            output: Output::Call(value),
            ..
        } => value,
        result => return Err(anyhow!("'getReserves' execution failed: {result:?}")),
    };

    // The output actually is u128,u128,u32, but we decode it as u256. Easier to handle.
    // Also we do not need the u32, which is the block timestamp.
    let (reserve0, reserve1, _) = <(U256, U256, u32)>::abi_decode(&value, false)
        .map_err(|_| anyhow!("Error decoding getReserves result"))?;

    Ok((reserve0, reserve1))
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
            tx.transact_to = TxKind::Call(UNISWAP_V2_ROUTER);
            tx.data = calldata.into();
        })
        .build();

    let tx = evm
        .transact()
        .map_err(|e| anyhow!("Failed to execute getAmountOut call: {e}"))?;
    let result = tx.result;

    let value = match result {
        ExecutionResult::Success {
            output: Output::Call(value),
            ..
        } => value,
        result => return Err(anyhow!("'getAmountOut' execution failed: {result:?}")),
    };

    let amount_out = <U256>::abi_decode(&value, false)
        .map_err(|_| anyhow!("Error decoding getAmountOut result"))?;

    Ok(amount_out)
}

pub fn univ2_swap(
    sender: Address,
    pair: &UniV2Pair,
    token_in: Address,
    amount_in: U256,
    reserves: (U256, U256),
    cache_db: &mut AlloyCacheDB,
) -> Result<U256> {
    let is_token_0_in = pair.token0 == token_in;
    let (reserve_in, reserve_out) = if is_token_0_in {
        reserves
    } else {
        (reserves.1, reserves.0)
    };

    transfer(sender, pair.address, amount_in, token_in, cache_db)?;
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

    let tx = evm
        .transact_commit()
        .map_err(|e| anyhow!("Failed to execute swap call on Uniswap V2 pair: {e}"))?;

    match tx {
        ExecutionResult::Success { .. } => {}
        result => {
            return Err(anyhow!(
                "'swap' execution failed on Uniswap V2 pair: {result:?}"
            ))
        }
    }

    Ok(amount_out)
}
