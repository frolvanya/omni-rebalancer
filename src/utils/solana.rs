use std::sync::Arc;

use derive_builder::Builder;
use omni_types::OmniAddress;
use thiserror::Error;

use url::Url;

use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;

use crate::utils::WATCHER_INTERVAL;

#[derive(Debug, Error)]
pub enum ClientError {
    #[error("Solana RPC error: {0}")]
    RpcError(#[from] solana_rpc_client_api::client_error::Error),

    #[error("Unsupported relayer address for EVM: {0}")]
    UnsupportedRelayerType(OmniAddress),
}

#[derive(Builder, Clone)]
#[builder(pattern = "owned")]
pub struct Client {
    pub client: Arc<RpcClient>,
    pub relayer: OmniAddress,
    pub threshold: u64,
}

impl Client {
    pub fn new(url: Url, relayer: OmniAddress, threshold: u64) -> Result<Self, ClientError> {
        Ok(Self {
            client: Arc::new(RpcClient::new(url.to_string())),
            relayer,
            threshold,
        })
    }

    pub async fn get_native_balance(&self, address: Pubkey) -> Result<u64, ClientError> {
        self.client
            .get_balance(&address)
            .await
            .map_err(ClientError::RpcError)
    }

    pub async fn balance_watcher(&self) -> Result<(), ClientError> {
        let OmniAddress::Sol(address) = &self.relayer else {
            return Err(ClientError::UnsupportedRelayerType(self.relayer.clone()));
        };
        let address = address.0.into();

        loop {
            let balance = self.get_native_balance(address).await?;

            if balance < self.threshold {
                tracing::warn!(
                    "Balance for address {} is below threshold: {} < {}",
                    self.relayer,
                    balance,
                    self.threshold
                );
            }

            tokio::time::sleep(tokio::time::Duration::from_secs(WATCHER_INTERVAL)).await;
        }
    }
}
