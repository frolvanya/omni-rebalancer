use solana_sdk::pubkey::Pubkey;
use thiserror::Error;

use alloy::primitives::U256;
use derive_builder::{Builder, UninitializedFieldError};
use omni_types::{ChainKind, OmniAddress};
use tokio::task::JoinHandle;

use crate::config::{self, Config};

pub mod evm;
pub mod near;
pub mod solana;

const WATCHER_INTERVAL: u64 = 60;

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

fn build_evm_client(opt: Option<config::Evm>) -> Result<Option<evm::Client>, ClientError> {
    opt.map(|cfg| evm::Client::new(cfg.rpc_url, cfg.relayer, cfg.threshold))
        .transpose()
        .map_err(ClientError::EvmError)
}

fn build_solana_client(opt: Option<config::Solana>) -> Result<Option<solana::Client>, ClientError> {
    opt.map(|cfg| solana::Client::new(cfg.rpc_url, cfg.relayer, cfg.threshold))
        .transpose()
        .map_err(ClientError::SolanaError)
}

impl Client {
    pub fn build(config: Config) -> Result<Self, ClientError> {
        ClientBuilder::default()
            .near_client(Some(near::Client::new(config.near.rpc_url)))
            .eth_client(build_evm_client(config.eth)?)
            .base_client(build_evm_client(config.base)?)
            .arb_client(build_evm_client(config.arb)?)
            .solana_client(build_solana_client(config.solana)?)
            .build()
    }

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

    pub async fn balance_watcher(&self) -> Vec<JoinHandle<Result<(), ClientError>>> {
        let mut handles = Vec::new();

        if let Some(eth_client) = self.eth_client.clone() {
            tracing::info!("Starting Eth balance watcher for {}", eth_client.relayer);

            handles.push(tokio::spawn(async move {
                eth_client
                    .balance_watcher()
                    .await
                    .map_err(ClientError::EvmError)
            }));
        }

        if let Some(base_client) = self.base_client.clone() {
            tracing::info!("Starting Base balance watcher for {}", base_client.relayer);

            handles.push(tokio::spawn(async move {
                base_client
                    .balance_watcher()
                    .await
                    .map_err(ClientError::EvmError)
            }));
        }

        if let Some(arb_client) = self.arb_client.clone() {
            tracing::info!("Starting Arb balance watcher for {}", arb_client.relayer);

            handles.push(tokio::spawn(async move {
                arb_client
                    .balance_watcher()
                    .await
                    .map_err(ClientError::EvmError)
            }));
        }

        if let Some(solana_client) = self.solana_client.clone() {
            tracing::info!(
                "Starting Solana balance watcher for {}",
                solana_client.relayer
            );

            handles.push(tokio::spawn(async move {
                solana_client
                    .balance_watcher()
                    .await
                    .map_err(ClientError::SolanaError)
            }));
        }

        handles
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
