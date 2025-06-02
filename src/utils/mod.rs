use std::sync::Arc;

use alloy::primitives::U256;
use bridge_connector_common::result::BridgeSdkError;
use derive_builder::{Builder, UninitializedFieldError};
use near_bridge_client::{NearBridgeClient, TransactionOptions};
use near_primitives::views::TxExecutionStatus;
use near_sdk::AccountId;
use omni_types::{ChainKind, OmniAddress};
use rust_decimal::{Decimal, prelude::FromPrimitive};
use thiserror::Error;
use tokio::{
    sync::mpsc::{Receiver, Sender},
    task::JoinHandle,
};

use crate::config::Config;

pub mod evm;
pub mod near;
pub mod omni_endpoint;
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
pub enum FeeError {
    #[error("Native token fee is prohibited: {0}")]
    NativeTokenFeeIsProhibited(AccountId),

    #[error("Transferred token fee is prohibited: {0}")]
    TransferredTokenFeeIsProhibited(AccountId),

    #[error("High fee error: {0}")]
    HighFeeError(String),
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

    #[error("Omni endpoint error: {0}")]
    OmniEndpointError(#[from] omni_endpoint::ClientError),

    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("Unsupported chain: {0}")]
    UnsupportedChain(String),

    #[error("Fee error: {0}")]
    FeeError(#[from] FeeError),

    #[error("Parse error: {0}")]
    ParseError(String),

    #[error("Uninitialized field: {0}")]
    UninitializedFieldError(#[from] UninitializedFieldError),
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
    pub omni_endpoint_client: Option<omni_endpoint::Client>,

    pub rebalancer_tx: Option<Sender<OmniAddress>>,
}

impl Client {
    pub async fn build(config: Config, tx: Sender<OmniAddress>) -> Result<Self, ClientError> {
        ClientBuilder::default()
            .near_bridge_client(Some(config.build_near_bridge_client()?))
            .near_client(Some(config.build_near_client()))
            .eth_client(config.build_evm_client(ChainKind::Eth)?)
            .base_client(config.build_evm_client(ChainKind::Base)?)
            .arb_client(config.build_evm_client(ChainKind::Arb)?)
            .solana_client(config.build_solana_client()?)
            .omni_endpoint_client(config.build_omni_endpoint_client())
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
        tracing::info!("Trying to rebalance to {}", receiver);

        let near_bridge_client = self.near_bridge_client()?;
        let near_client = self.near_client()?;
        let omni_endpoint_client = self.omni_endpoint_client()?;

        let native_token = near_bridge_client
            .get_native_token_id(receiver.get_chain())
            .await
            .map_err(ClientError::NearBridgeClientError)?;

        tracing::info!(
            "Native token for {:?} is {}",
            receiver.get_chain(),
            native_token
        );

        let fee = omni_endpoint_client
            .get_transfer_fee(
                &OmniAddress::Near(near_client.relayer.clone()),
                &receiver,
                &OmniAddress::Near(native_token.clone()),
            )
            .await?;

        let Some(native_token_fee) = fee.native_token_fee else {
            return Err(ClientError::FeeError(FeeError::NativeTokenFeeIsProhibited(
                native_token,
            )));
        };

        let Some(transferred_token_fee) = fee.transferred_token_fee else {
            return Err(ClientError::FeeError(
                FeeError::TransferredTokenFeeIsProhibited(native_token),
            ));
        };

        let usd_fee = Self::f64_to_decimal(fee.usd_fee, "transfer fee")?;

        if Some(usd_fee) > near_client.max_fee_usd {
            return Err(ClientError::FeeError(FeeError::HighFeeError(format!(
                "Transfer fee {} exceeds max fee {:?}",
                fee.usd_fee, near_client.max_fee_usd
            ))));
        }

        let balance = self.get_relayer_balance(&native_token).await?;
        let balance_decimal = Self::u128_to_decimal(balance, "relayer balance")?;
        let price_per_unit =
            Self::get_transferred_token_price_per_unit(usd_fee, transferred_token_fee.0)?;
        let balance_usd = balance_decimal * price_per_unit;

        if Some(balance_usd) < near_client.min_rebalance_usd {
            tracing::info!(
                "Balance {:.2} USD is below minimum rebalance amount {:?}, skipping",
                balance_usd,
                near_client.min_rebalance_usd
            );
            return Ok(());
        }

        tracing::info!(
            "Sending {balance} of {native_token} from {} to {receiver}",
            near_client.relayer
        );

        let hash = near_bridge_client
            .init_transfer(
                native_token.to_string(),
                balance,
                receiver,
                0,
                native_token_fee.0,
                TransactionOptions {
                    nonce: None,
                    wait_until: TxExecutionStatus::Included,
                    wait_final_outcome_timeout_sec: None,
                },
            )
            .await?;

        tracing::info!("Transfer initiated with hash: {}", hash);

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

    pub fn omni_endpoint_client(&self) -> Result<&omni_endpoint::Client, ClientError> {
        self.omni_endpoint_client.as_ref().ok_or_else(|| {
            ClientError::ConfigError("Omni endpoint client is not initialized".to_string())
        })
    }

    pub fn rebalancer_tx(&self) -> Result<&Sender<OmniAddress>, ClientError> {
        self.rebalancer_tx.as_ref().ok_or_else(|| {
            ClientError::ConfigError("Rebalance channel is not initialized".to_string())
        })
    }

    fn u128_to_decimal(value: u128, context: &str) -> Result<Decimal, ClientError> {
        Decimal::from_u128(value).ok_or_else(|| {
            ClientError::ParseError(format!("Failed to convert {context} to decimal: {value}"))
        })
    }

    fn f64_to_decimal(value: f64, context: &str) -> Result<Decimal, ClientError> {
        Decimal::from_f64(value).ok_or_else(|| {
            ClientError::ParseError(format!("Failed to convert {context} to decimal: {value}"))
        })
    }

    async fn get_relayer_balance(&self, token: &AccountId) -> Result<u128, ClientError> {
        let near_client = self.near_client()?;
        near_client
            .ft_balance_of(token, &near_client.relayer)
            .await
            .map_err(ClientError::NearError)
    }

    fn get_transferred_token_price_per_unit(
        usd_fee: Decimal,
        transferred_token_fee: u128,
    ) -> Result<Decimal, ClientError> {
        let transferred_token_fee =
            Self::u128_to_decimal(transferred_token_fee, "transferred token fee")?;
        Ok(usd_fee / transferred_token_fee)
    }
}
