use alloy_eips::BlockId;
use alloy_primitives::{Bytes, U256};
use alloy_rpc_types_eth::{state::StateOverride, transaction::TransactionRequest, BlockOverrides};
use jsonrpsee::{
    http_client::{HttpClient, HttpClientBuilder},
    proc_macros::rpc,
    rpc_params,
    types::{error::INTERNAL_ERROR_CODE, ErrorObject},
};
use jsonrpsee_core::{async_trait, client::ClientT, ClientError, RpcResult};

#[rpc(server, namespace = "eth")]
pub(crate) trait CallForwarderApi {
    /// Executes a new message call immediately without creating a transaction on the block chain.
    #[method(name = "call")]
    async fn call(
        &self,
        request: TransactionRequest,
        block_number: Option<BlockId>,
        state_overrides: Option<StateOverride>,
        block_overrides: Option<Box<BlockOverrides>>,
    ) -> RpcResult<Bytes>;

    /// Generates and returns an estimate of how much gas is necessary to allow the transaction to
    /// complete.
    #[method(name = "estimateGas")]
    async fn estimate_gas(
        &self,
        request: TransactionRequest,
        block_number: Option<BlockId>,
        state_override: Option<StateOverride>,
    ) -> RpcResult<U256>;
}

pub struct CallForwarderExt {
    client: HttpClient,
}

impl CallForwarderExt {
    pub fn new(upstream_rpc_url: String) -> Self {
        let client =
            HttpClientBuilder::default().build(upstream_rpc_url).expect("Failed to build client");

        Self { client }
    }
}

#[async_trait]
impl CallForwarderApiServer for CallForwarderExt {
    async fn call(
        &self,
        request: TransactionRequest,
        block_number: Option<BlockId>,
        state_overrides: Option<StateOverride>,
        block_overrides: Option<Box<BlockOverrides>>,
    ) -> RpcResult<Bytes> {
        let result = self
            .client
            .clone()
            .request(
                "eth_call",
                rpc_params![request, block_number, state_overrides, block_overrides],
            )
            .await
            .map_err(|e| match e {
                ClientError::Call(e) => e,
                _ => ErrorObject::owned(
                    INTERNAL_ERROR_CODE,
                    format!("Failed to call: {e:?}"),
                    Some(()),
                ),
            })?;
        Ok(result)
    }

    async fn estimate_gas(
        &self,
        request: TransactionRequest,
        block_number: Option<BlockId>,
        state_override: Option<StateOverride>,
    ) -> RpcResult<U256> {
        let result = self
            .client
            .clone()
            .request("eth_estimateGas", rpc_params![request, block_number, state_override])
            .await
            .map_err(|e| match e {
                ClientError::Call(e) => e,
                _ => ErrorObject::owned(
                    INTERNAL_ERROR_CODE,
                    format!("Failed to estimate gas: {e:?}"),
                    Some(()),
                ),
            })?;
        Ok(result)
    }
}
