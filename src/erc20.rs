use alloy::{
    primitives::Address,
    providers::{Provider, RootProvider},
    rpc::types::TransactionRequest,
    sol,
    sol_types::{SolCall, SolValue},
    transports::http::{Client, Http},
};
use revm::{
    primitives::{address, ExecutionResult, Output, TxKind, U256},
    Evm,
};

use crate::{
    error::{HPError, Result},
    AlloyCacheDB,
};

pub const WETH: Address = address!("c02aaa39b223fe8d0a0e5c4f27ead9083c756cc2");

sol! {
    function balanceOf(address account) public returns (uint256);
    function transfer(address to, uint amount) external returns (bool);
}

#[derive(Debug, Clone)]
pub struct ERC20 {
    pub address: Address,
    pub name: String,
    pub symbol: String,
    pub decimals: u8,
}

pub async fn get_erc20_info(token: &Address, client: &RootProvider<Http<Client>>) -> Result<ERC20> {
    sol! {
      function name() public view returns (string);
      function symbol() public view returns (string);
      function decimals() public view returns (uint8);
    }

    let name = nameCall {}.abi_encode();
    let symbol = symbolCall {}.abi_encode();
    let decimals = decimalsCall {}.abi_encode();

    let name = client
        .call(&TransactionRequest {
            to: Some(TxKind::Call(*token)),
            input: name.into(),
            ..Default::default()
        })
        .await
        .expect("Token isn't ERC20 token");
    let symbol = client
        .call(&TransactionRequest {
            to: Some(TxKind::Call(*token)),
            input: symbol.into(),
            ..Default::default()
        })
        .await
        .expect("Token isn't ERC20 token");
    let decimals = client
        .call(&TransactionRequest {
            to: Some(TxKind::Call(*token)),
            input: decimals.into(),
            ..Default::default()
        })
        .await
        .expect("Token isn't ERC20 token");

    let name = nameCall::abi_decode_returns(&name, false).map_err(HPError::error)?;
    let symbol = symbolCall::abi_decode_returns(&symbol, false).map_err(HPError::error)?;
    let decimals = decimalsCall::abi_decode_returns(&decimals, false).map_err(HPError::error)?;

    Ok(ERC20 {
        address: token.clone(),
        name: name._0,
        symbol: symbol._0,
        decimals: decimals._0,
    })
}

impl ERC20 {
    // pub fn new(address: Address, name: String, symbol: String, decimals: u8) -> Self {
    //     Self {
    //         address,
    //         name,
    //         symbol,
    //         decimals,
    //     }
    // }

    pub fn balance_of(
        &self,
        owner: Address,
        sender: Address,
        alloy_db: &mut AlloyCacheDB,
    ) -> Result<U256> {
        let encoded = balanceOfCall { account: owner }.abi_encode();

        let mut evm = Evm::builder()
            .with_db(alloy_db)
            .modify_tx_env(|tx| {
                // For consistency, we use the same sender for all calls
                tx.caller = sender;
                tx.transact_to = TxKind::Call(self.address);
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
        &self,
        from: Address,
        to: Address,
        amount: U256,
        // token: Address,
        alloy_db: &mut AlloyCacheDB,
    ) -> Result<()> {
        let calldata = transferCall { to, amount }.abi_encode();

        let mut evm = Evm::builder()
            .with_db(alloy_db)
            .modify_tx_env(|tx| {
                tx.caller = from;
                tx.transact_to = TxKind::Call(self.address);
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
}
