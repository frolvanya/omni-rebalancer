use alloy::primitives::U256;
use near_bridge_client::NearBridgeClient;
use near_sdk::AccountId;
use omni_types::{ChainKind, OmniAddress};
use rust_decimal::Decimal;
use serde::Deserialize;
use url::Url;

use crate::utils::{ClientError, evm, near, omni_endpoint, solana};

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
    pub omni_endpoint: Option<OmniEndpoint>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Near {
    pub rpc_url: Url,
    pub relayer: AccountId,
    pub omni_bridge_id: AccountId,
    pub max_fee_usd: Option<Decimal>,
    pub min_rebalance_usd: Option<Decimal>,
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

#[derive(Debug, Clone, Deserialize)]
pub struct OmniEndpoint {
    pub api_url: Url,
}

impl Config {
    pub fn build_near_bridge_client(&self) -> Result<NearBridgeClient, ClientError> {
        near_bridge_client::NearBridgeClientBuilder::default()
            .endpoint(Some(self.near.rpc_url.to_string()))
            .private_key(Some(std::env::var("NEAR_PRIVATE_KEY").map_err(|_| {
                ClientError::ConfigError(
                    "`NEAR_PRIVATE_KEY` environment variable is not set".to_string(),
                )
            })?))
            .signer(Some(self.near.relayer.to_string()))
            .omni_bridge_id(Some(self.near.omni_bridge_id.to_string()))
            .btc_connector(None)
            .build()
            .map_err(|err| ClientError::ConfigError(err.to_string()))
    }

    pub fn build_near_client(&self) -> near::Client {
        near::Client::new(
            self.near.rpc_url.clone(),
            self.near.relayer.clone(),
            self.near.max_fee_usd,
            self.near.min_rebalance_usd,
        )
    }

    pub fn build_evm_client(&self, chain: ChainKind) -> Result<Option<evm::Client>, ClientError> {
        let evm = match chain {
            ChainKind::Eth => self.eth.as_ref(),
            ChainKind::Base => self.base.as_ref(),
            ChainKind::Arb => self.arb.as_ref(),
            _ => {
                return Err(ClientError::UnsupportedChain(format!(
                    "Cannot build EVM client for {chain:?}"
                )));
            }
        };

        evm.map(|cfg| evm::Client::new(cfg.rpc_url.clone(), cfg.relayer.clone(), cfg.threshold))
            .transpose()
            .map_err(ClientError::EvmError)
    }

    pub fn build_solana_client(&self) -> Result<Option<solana::Client>, ClientError> {
        self.solana
            .as_ref()
            .map(|cfg| solana::Client::new(cfg.rpc_url.clone(), cfg.relayer.clone(), cfg.threshold))
            .transpose()
            .map_err(ClientError::SolanaError)
    }

    pub fn build_omni_endpoint_client(&self) -> Option<omni_endpoint::Client> {
        self.omni_endpoint
            .as_ref()
            .map(|cfg| omni_endpoint::Client::new(cfg.api_url.clone()))
    }
}
