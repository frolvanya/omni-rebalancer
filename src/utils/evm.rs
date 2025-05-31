use thiserror::Error;

use url::Url;

use alloy::{
    primitives::U256,
    providers::{DynProvider, Provider, ProviderBuilder},
};

#[derive(Debug, Error)]
pub enum ClientError {
    #[error("EVM client error: {0}")]
    TransportError(#[from] alloy::transports::TransportError),
}

#[derive(Clone, Debug)]
pub struct Client {
    client: DynProvider,
}

impl Client {
    pub fn new(url: Url) -> Self {
        Self {
            client: DynProvider::new(ProviderBuilder::new().connect_http(url)),
        }
    }

    #[tracing::instrument(name = "evm_get_native_balance", skip(self))]
    pub async fn get_native_balance(
        &self,
        address: alloy::primitives::Address,
    ) -> Result<U256, ClientError> {
        self.client
            .get_balance(address)
            .await
            .map_err(ClientError::TransportError)
    }
}
