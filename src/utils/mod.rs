use std::sync::Arc;

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

macro_rules! spawn_watcher {
    ($handles:ident, $self:ident, $client_opt:expr) => {
        if let Some(client) = &$client_opt {
            let relayer = client.relayer.clone();
            let threshold = client.threshold;
            let cloned_self = Arc::clone(&$self);

            $handles.push(tokio::spawn(async move {
                cloned_self
                    .start_relayer_balance_watcher(relayer, threshold)
                    .await
            }));
        }
    };
}

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

    pub async fn get_native_balance(
        &self,
        omni_address: &OmniAddress,
    ) -> Result<U256, ClientError> {
        match omni_address {
            OmniAddress::Near(account_id) => self
                .near_client()?
                .get_native_balance(account_id)
                .await
                .map(U256::from)
                .map_err(ClientError::NearError),
            OmniAddress::Eth(address) | OmniAddress::Base(address) | OmniAddress::Arb(address) => {
                self.evm_client(omni_address.get_chain())?
                    .get_native_balance(address.0.into())
                    .await
                    .map_err(ClientError::EvmError)
            }
            OmniAddress::Sol(address) => self
                .solana_client()?
                .get_native_balance(address.0.into())
                .await
                .map(U256::from)
                .map_err(ClientError::SolanaError),
        }
    }

    pub async fn start_relayer_balance_watcher(
        &self,
        relayer: OmniAddress,
        threshold: U256,
    ) -> Result<(), ClientError> {
        tracing::info!("Starting relayer balance watcher for {}", relayer);

        loop {
            let balance = self.get_native_balance(&relayer).await?;

            if balance < threshold {
                tracing::warn!(
                    "Balance for address {} is below threshold: {} < {}",
                    relayer,
                    balance,
                    threshold
                );
            }

            tokio::time::sleep(tokio::time::Duration::from_secs(WATCHER_INTERVAL)).await;
        }
    }

    pub async fn start_all_relayer_balance_watchers(
        self: Arc<Self>,
    ) -> Vec<JoinHandle<Result<(), ClientError>>> {
        let mut handles = Vec::new();

        spawn_watcher!(handles, self, self.eth_client);
        spawn_watcher!(handles, self, self.base_client);
        spawn_watcher!(handles, self, self.arb_client);
        spawn_watcher!(handles, self, self.solana_client);

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
