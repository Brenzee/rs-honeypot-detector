use alloy::{
    eips::BlockId,
    network::Ethereum,
    primitives::{address, Address},
    providers::RootProvider,
    transports::http::{Client, Http},
};
use clap::Parser;
use cli::{Cli, Protocol};

use crate::{
    error::{HPError, Result},
    test_swap::TestSwap,
};
use revm::db::{AlloyDB, CacheDB};
use uniswapv2::UniswapV2;
// use uniswapv3::test_swap;

mod cli;
mod erc20;
mod error;
mod revm_actions;
mod test_swap;
mod uniswapv2;
mod uniswapv3;

// TODO:
// - Take RPC_URL from env variables. Currently using local Reth node.
// - Improve swap error handling
// - Add more options to the CLI (more logs, more details about token, use specific sender)

const WETH: Address = address!("c02aaa39b223fe8d0a0e5c4f27ead9083c756cc2");

type AlloyProvider = RootProvider<Http<Client>>;
type AlloyCacheDB = CacheDB<AlloyDB<Http<Client>, Ethereum, AlloyProvider>>;

#[tokio::main]
async fn main() -> Result<()> {
    let config = Cli::parse().validate().await?;

    let mut cache_db = get_cache_db(config.client.clone())?;

    match config.protocol {
        Protocol::UniV2 => UniswapV2::test_swap(&config, &mut cache_db).await?,
        _ => return Err(HPError::err_msg("Unsupported protocol".to_string())),
    }

    Ok(())
}

fn get_cache_db(client: RootProvider<Http<Client>>) -> Result<AlloyCacheDB> {
    let db = AlloyDB::new(client, BlockId::latest()).expect("Failed to create Revm Alloy DB");
    Ok(CacheDB::new(db))
}
