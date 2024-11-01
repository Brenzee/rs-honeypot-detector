use alloy::{
    sol,
    sol_types::{SolCall, SolValue},
};
// use anyhow::{anyhow, Result};
use revm::{
    primitives::{Address, ExecutionResult, Output, TxKind, U256},
    Evm,
};

use crate::{
    error::{HPError, Result},
    AlloyCacheDB,
};

// Have a single sol! macro for all the functions we need to call
sol! {
    function balanceOf(address account) public returns (uint256);
    function transfer(address to, uint amount) external returns (bool);
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

    let tx = evm.transact().map_err(HPError::error)?;

    let result = tx.result;
    let balance = match result {
        ExecutionResult::Success {
            output: Output::Call(value),
            ..
        } => value,
        result => {
            return Err(HPError::new(
                format!("'balanceOf' execution failed: {result:?}"),
                None,
            ))
        }
    };

    let balance = <U256>::abi_decode(&balance, false).map_err(HPError::error)?;

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

    let tx = evm.transact_commit().map_err(HPError::error)?;

    let res = match tx {
        ExecutionResult::Success {
            output: Output::Call(value),
            ..
        } => value,
        result => {
            return Err(HPError::new(
                format!("'transfer' execution failed: {result:?}"),
                None,
            ))
        }
    };

    // Some tokens do not return a boolean, so we check if the result is empty
    // If it is empty, we consider the transfer successful, because if it would have failed,
    // the transaction would have failed.
    if res.len() == 0 {
        return Ok(());
    }

    let is_success = <bool>::abi_decode(&res, false).map_err(HPError::error)?;

    if !is_success {
        return Err(HPError::new("'transfer' failed".to_string(), None));
    }

    Ok(())
}
