use alloy::{
    providers::{Provider, ProviderBuilder},
    transports::http::reqwest::Url,
};
use anyhow::Result;
use clap::Parser;
use revm::primitives::{address, Address};

use crate::{
    erc20::{get_erc20_info, ERC20},
    error::HPError,
    AlloyProvider, WETH,
};

pub const DEFAULT_ACC: Address = address!("e4A6aD6E1B86AB8f2d2f571717592De46bFaF614");

#[derive(clap::ValueEnum, Clone, Debug, Copy)]
pub enum Protocol {
    UniV2,
    UniV3,
}

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
#[command(next_line_help = true)]
pub struct Cli {
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

    /// The protocol used to test the token
    #[arg(short, long, value_enum, default_value_t = Protocol::UniV2)]
    protocol: Protocol,
}

#[derive(Debug)]
pub struct CliConfig {
    pub from_token: ERC20,
    pub token: ERC20,
    pub logs: bool,
    pub sender: Address,
    pub client: AlloyProvider,
    pub protocol: Protocol,
}

impl Cli {
    pub async fn validate(&self) -> Result<CliConfig, HPError> {
        let token: Address = self.token.parse().map_err(HPError::parse_error)?;

        let sender = if let Some(sender) = self.sender.as_ref() {
            sender.parse().map_err(HPError::parse_error)?
        } else {
            DEFAULT_ACC
        };

        let rpc_url = Url::parse(&self.rpc_url).map_err(|e| HPError::new(e.to_string(), None))?;
        let client = ProviderBuilder::new().on_http(rpc_url.clone());

        let chain_id = client.get_chain_id().await.map_err(HPError::rpc_error)?;
        if chain_id != 1 {
            return Err(HPError::err_msg(format!(
                "Only mainnet is supported, the provided RPC URL is for chain {}",
                chain_id
            )));
        }

        let from_token = ERC20 {
            address: WETH,
            name: "Wrapped Ether".to_string(),
            symbol: "WETH".to_string(),
            decimals: 18,
        };

        let token = get_erc20_info(&token, &client)
            .await
            .map_err(HPError::rpc_error)?;

        Ok(CliConfig {
            from_token,
            token,
            logs: self.logs,
            sender,
            client: client.clone(),
            protocol: self.protocol,
        })
    }
}
