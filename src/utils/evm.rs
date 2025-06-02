use alloy::{
    primitives::U256,
    providers::{DynProvider, Provider, ProviderBuilder},
};
use omni_types::OmniAddress;
use thiserror::Error;
use url::Url;

#[derive(Debug, Error)]
pub enum ClientError {
    #[error("EVM client error: {0}")]
    TransportError(#[from] alloy::transports::TransportError),
}

#[derive(Clone, Debug)]
pub struct Client {
    pub client: DynProvider,
    pub relayer: OmniAddress,
    pub threshold: U256,
}

impl Client {
    pub fn new(url: Url, relayer: OmniAddress, threshold: U256) -> Result<Self, ClientError> {
        Ok(Self {
            client: DynProvider::new(ProviderBuilder::new().connect_http(url)),
            relayer,
            threshold,
        })
    }

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
