use std::sync::Arc;

use bridge_connector_common::result::BridgeSdkError;
use near_bridge_client::NearBridgeClient;
use thiserror::Error;

use alloy::primitives::U256;
use derive_builder::{Builder, UninitializedFieldError};
use omni_types::{ChainKind, OmniAddress};
use tokio::{
    sync::mpsc::{Receiver, Sender},
    task::JoinHandle,
};

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
            let cloned_self = $self.clone();

            $handles.push(tokio::spawn(async move {
                cloned_self
                    .start_relayer_balance_watcher(relayer, threshold)
                    .await
            }));
        }
    };
}

#[allow(clippy::enum_variant_names)]
#[derive(Debug, Error)]
pub enum ClientError {
    #[error("Near Bridge Client error: {0}")]
    NearBridgeClientError(#[from] BridgeSdkError),

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
    pub near_bridge_client: Option<NearBridgeClient>,

    pub near_client: Option<near::Client>,
    pub eth_client: Option<evm::Client>,
    pub base_client: Option<evm::Client>,
    pub arb_client: Option<evm::Client>,
    pub solana_client: Option<solana::Client>,

    pub rebalancer_tx: Option<Sender<OmniAddress>>,
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
    pub async fn build(config: Config, tx: Sender<OmniAddress>) -> Result<Self, ClientError> {
        let near_bridge_client = near_bridge_client::NearBridgeClientBuilder::default()
            .endpoint(Some(config.near.rpc_url.to_string()))
            .private_key(Some(std::env::var("NEAR_PRIVATE_KEY").map_err(|_| {
                ClientError::ConfigError(
                    "`NEAR_PRIVATE_KEY` environment variable is not set".to_string(),
                )
            })?))
            .signer(Some(config.near.relayer.to_string()))
            .omni_bridge_id(Some(config.near.omni_bridge_id.to_string()))
            .btc_connector(None)
            .build()
            .map_err(|err| ClientError::ConfigError(err.to_string()))?;

        ClientBuilder::default()
            .near_bridge_client(Some(near_bridge_client))
            .near_client(Some(near::Client::new(
                config.near.rpc_url,
                config.near.relayer,
            )))
            .eth_client(build_evm_client(config.eth)?)
            .base_client(build_evm_client(config.base)?)
            .arb_client(build_evm_client(config.arb)?)
            .solana_client(build_solana_client(config.solana)?)
            .rebalancer_tx(Some(tx))
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
                if let Err(err) = self.rebalancer_tx()?.try_send(relayer.clone()) {
                    tracing::warn!("Failed to send rebalance request for {}: {}", relayer, err);
                }
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

    pub async fn rebalance(&self, receiver: OmniAddress) -> Result<(), ClientError> {
        let near_client = self.near_client()?;
        let near_bridge_client = self.near_bridge_client()?;

        let native_token = near_bridge_client
            .get_native_token_id(receiver.get_chain())
            .await
            .map_err(ClientError::NearBridgeClientError)?;

        let balance = near_client
            .ft_balance_of(&native_token, &near_client.relayer)
            .await?;

        tracing::info!(
            "Sending {balance} of {native_token} from {} to {receiver}",
            near_client.relayer
        );

        Ok(())
    }

    pub async fn start_rebalancer(
        self: Arc<Self>,
        mut rebalancer_rx: Receiver<OmniAddress>,
    ) -> JoinHandle<Result<(), ClientError>> {
        tracing::info!("Starting rebalancer");

        tokio::spawn(async move {
            while let Some(address) = rebalancer_rx.recv().await {
                tokio::spawn({
                    let cloned_self = self.clone();

                    async move {
                        if let Err(err) = cloned_self.rebalance(address.clone()).await {
                            tracing::error!("Rebalance failed for {}: {}", address, err);
                        }
                    }
                });
            }

            Ok(())
        })
    }

    pub fn near_bridge_client(&self) -> Result<&NearBridgeClient, ClientError> {
        self.near_bridge_client.as_ref().ok_or_else(|| {
            ClientError::ConfigError("Near bridge client is not initialized".to_string())
        })
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

    pub fn rebalancer_tx(&self) -> Result<&Sender<OmniAddress>, ClientError> {
        self.rebalancer_tx.as_ref().ok_or_else(|| {
            ClientError::ConfigError("Rebalance channel is not initialized".to_string())
        })
    }
}

impl From<UninitializedFieldError> for ClientError {
    fn from(err: UninitializedFieldError) -> Self {
        ClientError::ConfigError(format!("Missing field: {}", err))
    }
}
