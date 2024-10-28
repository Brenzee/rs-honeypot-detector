use alloy::{
    primitives::Address,
    providers::{Provider, RootProvider},
    rpc::types::TransactionRequest,
    sol,
    sol_types::SolCall,
    transports::http::{Client, Http},
};
use anyhow::Result;
use revm::primitives::TxKind;

#[derive(Debug)]
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

    let name = nameCall::abi_decode_returns(&name, false)?;
    let symbol = symbolCall::abi_decode_returns(&symbol, false)?;
    let decimals = decimalsCall::abi_decode_returns(&decimals, false)?;

    Ok(ERC20 {
        address: token.clone(),
        name: name._0,
        symbol: symbol._0,
        decimals: decimals._0,
    })
}
