use alloy::{
    primitives::Address,
    providers::{Provider, RootProvider},
    rpc::types::TransactionRequest,
    sol,
    sol_types::SolCall,
    transports::http::{Client, Http},
};
use anyhow::{anyhow, Result};
use revm::primitives::{address, TxKind};

const UNIV2_FACTORY: Address = address!("5C69bEe701ef814a2B6a3EDD4B1652CB9cc5aA6f");
const WETH: Address = address!("c02aaa39b223fe8d0a0e5c4f27ead9083c756cc2");

#[derive(Debug)]
pub struct UniV2Pair {
    pub address: Address,
    pub token0: Address,
    pub token1: Address,
}

pub async fn get_weth_pair(
    token: &Address,
    client: &RootProvider<Http<Client>>,
) -> Result<UniV2Pair> {
    sol! {
      function getPair(address,address) public view returns (address);
    }

    let pair_calldata = getPairCall {
        _0: *token,
        _1: WETH,
    }
    .abi_encode();

    let pair = client
        .call(&TransactionRequest {
            to: Some(TxKind::Call(UNIV2_FACTORY)),
            input: pair_calldata.into(),
            ..Default::default()
        })
        .await
        .expect("Token isn't ERC20 token");

    let pair_res = getPairCall::abi_decode_returns(&pair, true)?._0;

    if pair_res == Address::ZERO {
        return Err(anyhow!("Pair does not exist on Uniswap V2"));
    }

    let token0 = if *token < WETH { token.clone() } else { WETH };
    let token1 = if token0 == WETH { token.clone() } else { WETH };

    Ok(UniV2Pair {
        address: pair_res,
        token0,
        token1,
    })
}
