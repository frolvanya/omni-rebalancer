use omni_types::OmniAddress;
use thiserror::Error;

use url::Url;

use alloy::{
    primitives::U256,
    providers::{DynProvider, Provider, ProviderBuilder},
};

use crate::utils::WATCHER_INTERVAL;

#[derive(Debug, Error)]
pub enum ClientError {
    #[error("EVM client error: {0}")]
    TransportError(#[from] alloy::transports::TransportError),

    #[error("Unsupported relayer address for EVM: {0}")]
    UnsupportedRelayerType(OmniAddress),
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

    pub async fn balance_watcher(&self) -> Result<(), ClientError> {
        let (OmniAddress::Eth(address) | OmniAddress::Base(address) | OmniAddress::Arb(address)) =
            &self.relayer
        else {
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
