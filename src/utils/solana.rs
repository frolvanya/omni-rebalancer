use std::sync::Arc;

use alloy::primitives::U256;
use derive_builder::Builder;
use omni_types::OmniAddress;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use thiserror::Error;
use url::Url;

#[derive(Debug, Error)]
pub enum ClientError {
    #[error("Solana RPC error: {0}")]
    RpcError(#[from] solana_rpc_client_api::client_error::Error),
}

#[derive(Builder, Clone)]
#[builder(pattern = "owned")]
pub struct Client {
    pub client: Arc<RpcClient>,
    pub relayer: OmniAddress,
    pub threshold: U256,
}

impl Client {
    pub fn new(url: Url, relayer: OmniAddress, threshold: U256) -> Result<Self, ClientError> {
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
}
