use anyhow::Result;
use thiserror::Error;

use url::Url;

use near_jsonrpc_client::{
    JsonRpcClient,
    errors::JsonRpcError,
    methods::query::{RpcQueryError, RpcQueryRequest},
};
use near_jsonrpc_primitives::types::query::QueryResponseKind;
use near_primitives::{
    types::{BlockReference, Finality},
    views::{AccountView, QueryRequest},
};
use near_sdk::AccountId;

#[derive(Debug, Error)]
pub enum ClientError {
    #[error("Rpc query error: {0}")]
    RpcQueryError(#[from] JsonRpcError<RpcQueryError>),

    #[error("Unexpected response kind: {0:?}")]
    UnexpectedResponseKind(QueryResponseKind),
}

#[derive(Clone, Debug)]
pub struct Client {
    pub client: JsonRpcClient,
}

impl Client {
    pub fn new(url: Url) -> Self {
        Self {
            client: JsonRpcClient::connect(url),
        }
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

    #[tracing::instrument(name = "near_get_native_balance", skip(self))]
    pub async fn get_native_balance(&self, account_id: &AccountId) -> Result<u128, ClientError> {
        self.view_account(account_id)
            .await
            .map(|account| account.amount)
    }
}
