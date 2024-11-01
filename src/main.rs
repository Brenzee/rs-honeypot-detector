use alloy::{
    eips::BlockId,
    network::Ethereum,
    providers::RootProvider,
    transports::http::{Client, Http},
};
use clap::Parser;
use cli::{Cli, CliConfig, Protocol};

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
mod test_swap;
mod uniswapv2;
mod uniswapv3;

type AlloyProvider = RootProvider<Http<Client>>;
type AlloyCacheDB = CacheDB<AlloyDB<Http<Client>, Ethereum, AlloyProvider>>;

#[tokio::main]
async fn main() -> Result<()> {
    let config = Cli::parse().validate().await?;

    let mut cache_db = get_cache_db(config.client.clone())?;

    let protocol = match config.protocol {
        Protocol::UniV2 => UniswapV2::new(),
        _ => return Err(HPError::err_msg("Unsupported protocol".to_string())),
    };

    do_test_swap(protocol, &config, &mut cache_db).await?;

    println!("\n Successful Swap \n");

    Ok(())
}

async fn do_test_swap(
    protocol: impl TestSwap,
    config: &CliConfig,
    db: &mut AlloyCacheDB,
) -> Result<()> {
    protocol.test_swap(config, db).await
}

fn get_cache_db(client: RootProvider<Http<Client>>) -> Result<AlloyCacheDB> {
    let db = AlloyDB::new(client, BlockId::latest()).expect("Failed to create Revm Alloy DB");
    Ok(CacheDB::new(db))
}
