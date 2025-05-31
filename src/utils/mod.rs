use solana_sdk::pubkey::Pubkey;
use thiserror::Error;

use alloy::primitives::U256;
use derive_builder::{Builder, UninitializedFieldError};
use omni_types::{ChainKind, OmniAddress};

use crate::config::Config;

pub mod evm;
pub mod near;
pub mod solana;

#[derive(Debug, Error)]
pub enum ClientError {
    #[error("Near client error: {0}")]
    NearError(#[from] near::ClientError),

    #[error("EVM client error: {0}")]
    EvmError(#[from] evm::ClientError),

    #[error("Solana client error: {0}")]
    SolanaError(#[from] solana::ClientError),

    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("Unsupported chain: {0}")]
    UnsupportedChain(String),
}

#[derive(Builder, Default)]
#[builder(pattern = "owned", build_fn(error = "ClientError"))]
pub struct Client {
    pub near_client: Option<near::Client>,
    pub eth_client: Option<evm::Client>,
    pub base_client: Option<evm::Client>,
    pub arb_client: Option<evm::Client>,
    pub solana_client: Option<solana::Client>,
}

impl Client {
    pub fn build(config: Config) -> Result<Self, ClientError> {
        let near_client = near::Client::new(config.near.rpc_url);
        let eth_client = config.eth.map(|eth| evm::Client::new(eth.rpc_url));
        let base_client = config.base.map(|base| evm::Client::new(base.rpc_url));
        let arb_client = config.arb.map(|arb| evm::Client::new(arb.rpc_url));
        let solana_client = config
            .solana
            .map(|solana| solana::Client::new(solana.rpc_url));

        ClientBuilder::default()
            .near_client(Some(near_client))
            .eth_client(eth_client)
            .base_client(base_client)
            .arb_client(arb_client)
            .solana_client(solana_client)
            .build()
    }

    #[tracing::instrument(name = "get_native_balance", skip(self))]
    pub async fn get_native_balance(&self, omni_address: OmniAddress) -> Result<U256, ClientError> {
        match &omni_address {
            OmniAddress::Near(account_id) => self
                .near_client()?
                .get_native_balance(account_id)
                .await
                .map_err(ClientError::NearError)
                .map(U256::from),
            OmniAddress::Eth(address) | OmniAddress::Base(address) | OmniAddress::Arb(address) => {
                self.evm_client(omni_address.get_chain())?
                    .get_native_balance(address.0.into())
                    .await
                    .map_err(ClientError::EvmError)
            }
            OmniAddress::Sol(address) => self
                .solana_client()?
                .get_native_balance(Pubkey::from(address.0))
                .await
                .map_err(ClientError::SolanaError)
                .map(U256::from),
        }
    }

    pub fn near_client(&self) -> Result<&near::Client, ClientError> {
        self.near_client
            .as_ref()
            .ok_or_else(|| ClientError::ConfigError("Near client is not initialized".to_string()))
    }

    pub fn evm_client(&self, chain: ChainKind) -> Result<&evm::Client, ClientError> {
        match chain {
            ChainKind::Eth => self.eth_client.as_ref(),
            ChainKind::Base => self.base_client.as_ref(),
            ChainKind::Arb => self.arb_client.as_ref(),
            _ => {
                return Err(ClientError::UnsupportedChain(
                    "Unsupported EVM chain".to_string(),
                ));
            }
        }
        .ok_or_else(|| ClientError::ConfigError("EVM client is not initialized".to_string()))
    }

    pub fn solana_client(&self) -> Result<&solana::Client, ClientError> {
        self.solana_client
            .as_ref()
            .ok_or_else(|| ClientError::ConfigError("Solana client is not initialized".to_string()))
    }
}

impl From<UninitializedFieldError> for ClientError {
    fn from(err: UninitializedFieldError) -> Self {
        ClientError::ConfigError(format!("Missing field: {}", err))
    }
}
