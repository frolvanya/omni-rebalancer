use anyhow::Result;
use serde_json::json;
use thiserror::Error;

use url::Url;

use near_jsonrpc_client::{
    JsonRpcClient,
    errors::JsonRpcError,
    methods::query::{RpcQueryError, RpcQueryRequest},
};
use near_jsonrpc_primitives::types::query::QueryResponseKind;
use near_primitives::{
    types::{BlockReference, Finality, FunctionArgs},
    views::{AccountView, QueryRequest},
};
use near_sdk::AccountId;

#[derive(Clone)]
pub struct ViewRequest {
    pub contract_account_id: AccountId,
    pub method_name: String,
    pub args: serde_json::Value,
}

#[derive(Debug, Error)]
pub enum ClientError {
    #[error("Rpc query error: {0}")]
    RpcQueryError(#[from] JsonRpcError<RpcQueryError>),

    #[error("Unexpected response kind: {0:?}")]
    UnexpectedResponseKind(QueryResponseKind),

    #[error("Failed to parse response: {0}")]
    UnexpectedResponse(String),
}

#[derive(Clone, Debug)]
pub struct Client {
    pub client: JsonRpcClient,
    pub relayer: AccountId,
}

impl Client {
    pub fn new(url: Url, relayer: AccountId) -> Self {
        Self {
            client: JsonRpcClient::connect(url),
            relayer,
        }
    }

    pub async fn view_call(&self, view_request: ViewRequest) -> Result<Vec<u8>, ClientError> {
        let request = RpcQueryRequest {
            block_reference: BlockReference::Finality(Finality::Final),
            request: QueryRequest::CallFunction {
                account_id: view_request.contract_account_id,
                method_name: view_request.method_name,
                args: FunctionArgs::from(view_request.args.to_string().into_bytes()),
            },
        };

        let response = self.client.call(request).await?;

        if let QueryResponseKind::CallResult(result) = response.kind {
            Ok(result.result)
        } else {
            Err(ClientError::UnexpectedResponseKind(response.kind))
        }
    }

    pub async fn ft_balance_of(
        &self,
        token: &AccountId,
        account_id: &AccountId,
    ) -> Result<u128, ClientError> {
        let response = self
            .view_call(ViewRequest {
                contract_account_id: token.clone(),
                method_name: "ft_balance_of".to_string(),
                args: json!({ "account_id": account_id }),
            })
            .await?;

        serde_json::from_slice::<String>(&response)
            .map_err(|err| {
                ClientError::UnexpectedResponse(format!(
                    "Failed to parse balance response: {}",
                    err
                ))
            })?
            .parse()
            .map_err(|err| {
                ClientError::UnexpectedResponse(format!("Failed to parse balance: {}", err))
            })
    }

    pub async fn view_account(&self, account_id: &AccountId) -> Result<AccountView, ClientError> {
        let request = RpcQueryRequest {
            block_reference: BlockReference::Finality(Finality::Final),
            request: QueryRequest::ViewAccount {
                account_id: account_id.clone(),
            },
        };

        let response = self.client.call(request).await?;

        if let QueryResponseKind::ViewAccount(result) = response.kind {
            Ok(result)
        } else {
            Err(ClientError::UnexpectedResponseKind(response.kind))
        }
    }

    pub async fn get_native_balance(&self, account_id: &AccountId) -> Result<u128, ClientError> {
        self.view_account(account_id)
            .await
            .map(|account| account.amount)
    }
}
