use alloy::primitives::U256;
use near_sdk::AccountId;
use serde::Deserialize;
use url::Url;

use omni_types::OmniAddress;

fn replace_rpc_api_key<'de, D>(deserializer: D) -> Result<Url, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let url = String::deserialize(deserializer)?;

    let api_key = std::env::var("INFURA_API_KEY").map_err(serde::de::Error::custom)?;

    url.replace("INFURA_API_KEY", &api_key)
        .parse()
        .map_err(serde::de::Error::custom)
}

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub near: Near,
    pub eth: Option<Evm>,
    pub base: Option<Evm>,
    pub arb: Option<Evm>,
    pub solana: Option<Solana>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Near {
    pub rpc_url: Url,
    pub relayer: AccountId,
    pub omni_bridge_id: AccountId,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Evm {
    #[serde(deserialize_with = "replace_rpc_api_key")]
    pub rpc_url: Url,
    pub relayer: OmniAddress,
    pub threshold: U256,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Solana {
    pub rpc_url: Url,
    pub relayer: OmniAddress,
    pub threshold: U256,
}
