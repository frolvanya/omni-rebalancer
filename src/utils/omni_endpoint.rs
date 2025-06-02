use near_sdk::json_types::U128;
use omni_types::OmniAddress;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use url::Url;

#[derive(Debug, Error)]
pub enum ClientError {
    #[error("Failed to parse URL: {0}")]
    UrlParseError(#[from] url::ParseError),

    #[error("Request error: {0}")]
    RequestError(#[from] reqwest::Error),
}

#[derive(Clone, Debug)]
pub struct Client {
    pub url: Url,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TransferFee {
    pub native_token_fee: Option<U128>,
    pub transferred_token_fee: Option<U128>,
    pub usd_fee: f64,
}

impl Client {
    pub fn new(url: Url) -> Self {
        Self { url }
    }

    pub async fn get_transfer_fee(
        &self,
        sender: &OmniAddress,
        recipient: &OmniAddress,
        token: &OmniAddress,
    ) -> Result<TransferFee, ClientError> {
        let url = self.url.join(&format!(
            "/api/v1/transfer-fee?sender={}&recipient={}&token={}",
            sender, recipient, token
        ))?;

        reqwest::get(url)
            .await?
            .json()
            .await
            .map_err(ClientError::RequestError)
    }
}
