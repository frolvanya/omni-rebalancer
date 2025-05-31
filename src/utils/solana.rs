use derive_builder::Builder;
use thiserror::Error;

use url::Url;

use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;

#[derive(Debug, Error)]
pub enum ClientError {
    #[error("Solana RPC error: {0}")]
    RpcError(#[from] solana_rpc_client_api::client_error::Error),
}

#[derive(Builder)]
#[builder(pattern = "owned")]
pub struct Client {
    client: RpcClient,
}

impl Client {
    pub fn new(url: Url) -> Self {
        Self {
            client: RpcClient::new(url.to_string()),
        }
    }

    #[tracing::instrument(name = "solana_get_native_balance", skip(self))]
    pub async fn get_native_balance(&self, address: Pubkey) -> Result<u64, ClientError> {
        self.client
            .get_balance(&address)
            .await
            .map_err(ClientError::RpcError)
    }
}
