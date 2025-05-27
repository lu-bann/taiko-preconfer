use alloy_json_rpc::RpcRecv;
use alloy_rpc_client::RpcClient as AlloyClient;
use alloy_rpc_types::{Block, BlockNumberOrTag};
use serde_json::json;

use crate::http_client::{HttpClient, HttpError};

const GET_BLOCK_BY_NUMBER: &str = "eth_getBlockByNumber";

pub struct RpcClient {
    rpc_client: AlloyClient,
}

impl RpcClient {
    pub fn new(client: AlloyClient) -> Self {
        Self { rpc_client: client }
    }
}

impl HttpClient for RpcClient {
    async fn get<Resp: RpcRecv>(
        &self,
        method: String,
        params: serde_json::Value,
    ) -> Result<Resp, HttpError> {
        Ok(self.rpc_client.request(method, params).await?)
    }
}

pub async fn get_block(
    client: &RpcClient,
    block_number: BlockNumberOrTag,
    full_tx: bool,
) -> Result<Block, HttpError> {
    let params = json!([block_number, full_tx]);
    let block: Option<Block> = client.get(GET_BLOCK_BY_NUMBER.to_string(), params.clone()).await?;
    block.ok_or(HttpError::Rpc(alloy_json_rpc::RpcError::NullResp))
}

#[cfg(test)]
mod tests {
    use alloy_primitives::{B256, hex::FromHex};
    use alloy_rpc_types::BlockTransactions;

    use super::*;
    use crate::http_client::MockHttpClient;

    #[tokio::test]
    async fn test_get_block() {
        let mut client = MockHttpClient::new();

        let first_hash =
            B256::from_hex("0xb57c361f4e5fd7a2b6fc2246502c5fefd532217d0d5e7ffb75c841a7433914ad")
                .unwrap();
        let expected_first_hash = first_hash;
        let second_hash =
            B256::from_hex("0xc2a716e6782e1efa88f8f204eb202005cebe3f4e7b6109b3bd300367545ef69a")
                .unwrap();
        client.expect_get::<Block>().return_once(move |_, _| {
            let block_transactions = vec![first_hash, second_hash];
            Box::pin(async {
                Ok(Block::default()
                    .with_transactions(BlockTransactions::Hashes(block_transactions)))
            })
        });

        let method = GET_BLOCK_BY_NUMBER.to_string();
        let params = json!([BlockNumberOrTag::Latest, false]);
        let block: Block = client.get(method, params).await.unwrap();
        assert_eq!(block.transactions.hashes().next().unwrap(), expected_first_hash);
        assert_eq!(block.transactions.len(), 2);
    }
}
