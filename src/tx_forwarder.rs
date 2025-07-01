use alloy_primitives::{Bytes, B256};
use jsonrpsee::{
    http_client::{HttpClient, HttpClientBuilder},
    proc_macros::rpc,
    types::{error::INTERNAL_ERROR_CODE, ErrorObject},
};
use jsonrpsee_core::{async_trait, client::ClientT, ClientError, RpcResult};

#[rpc(server, namespace = "eth")]
pub trait EthForwarderApi {
    #[method(name = "sendRawTransaction")]
    async fn send_raw_transaction(&self, tx: Bytes) -> RpcResult<B256>;
}

pub struct EthForwarderExt {
    client: HttpClient,
}

impl EthForwarderExt {
    pub fn new(upstream_rpc_url: String) -> Self {
        let client =
            HttpClientBuilder::default().build(upstream_rpc_url).expect("Failed to build client");

        Self { client }
    }
}

#[async_trait]
impl EthForwarderApiServer for EthForwarderExt {
    async fn send_raw_transaction(&self, tx: Bytes) -> RpcResult<B256> {
        let txhash =
            self.client.clone().request("eth_sendRawTransaction", vec![tx]).await.map_err(|e| {
                match e {
                    ClientError::Call(e) => e,
                    _ => ErrorObject::owned(
                        INTERNAL_ERROR_CODE,
                        format!("Failed to send transaction: {e:?}"),
                        Some(()),
                    ),
                }
            })?;
        Ok(txhash)
    }
}
