/*
 * Copyright 2018-2020 TON DEV SOLUTIONS LTD.
 *
 * Licensed under the SOFTWARE EVALUATION License (the "License"); you may not use
 * this file except in compliance with the License.
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific TON DEV software governing permissions and
 * limitations under the License.
 *
 */

use super::blocks_walking::find_last_shard_block;
use crate::abi::Abi;
use crate::boc::internal::{deserialize_object_from_boc, DeserializedObject};
use crate::client::ClientContext;
use crate::encoding::{base64_decode, hex_decode};
use crate::error::{AddNetworkUrl, ClientResult};
use crate::net::Endpoint;
use crate::processing::internal::get_message_expiration_time;
use crate::processing::types::ProcessingEvent;
use crate::processing::Error;
use rand::seq::SliceRandom;
use std::sync::Arc;
use ton_block::{Message, MsgAddressInt};

#[derive(Serialize, Deserialize, ApiType, Default, Debug, Clone)]
pub struct ParamsOfSendMessage {
    /// Message BOC.
    pub message: String,

    /// Optional message ABI.
    ///
    /// If this parameter is specified and the message has the
    /// `expire` header then expiration time will be checked against
    /// the current time to prevent unnecessary sending of already expired message.
    ///
    /// The `message already expired` error will be returned in this
    /// case.
    ///
    /// Note, that specifying `abi` for ABI compliant contracts is
    /// strongly recommended, so that proper processing strategy can be
    /// chosen.
    pub abi: Option<Abi>,

    /// Flag for requesting events sending
    pub send_events: bool,
}

#[derive(Serialize, Deserialize, ApiType, Default, PartialEq, Debug)]
pub struct ResultOfSendMessage {
    /// The last generated shard block of the message destination account before the
    /// message was sent.
    ///
    /// This block id must be used as a parameter of the
    /// `wait_for_transaction`.
    pub shard_block_id: String,
}

#[derive(Clone)]
struct SendingMessage {
    serialized: String,
    deserialized: DeserializedObject<Message>,
    id: String,
    body: Vec<u8>,
    dst: MsgAddressInt,
}

impl SendingMessage {
    async fn new(
        context: &Arc<ClientContext>,
        serialized: &str,
        abi: Option<&Abi>,
    ) -> ClientResult<Self> {
        // Check message
        let deserialized =
            deserialize_object_from_boc::<ton_block::Message>(&context, serialized, "message")
                .await?;
        let id = deserialized.cell.repr_hash().to_hex_string();
        let dst = deserialized
            .object
            .dst()
            .ok_or(Error::message_has_not_destination_address())?;

        let message_expiration_time =
            get_message_expiration_time(context.clone(), abi, &serialized).await?;
        if let Some(message_expiration_time) = message_expiration_time {
            if message_expiration_time <= context.env.now_ms() {
                return Err(Error::message_already_expired());
            }
        }
        let body = base64_decode(serialized)?;
        Ok(Self {
            serialized: serialized.to_string(),
            deserialized,
            id,
            body,
            dst,
        })
    }

    async fn send_to_endpoint<F: futures::Future<Output = ()> + Send>(
        &self,
        context: Arc<ClientContext>,
        endpoint_address: &str,
        callback: Option<impl Fn(ProcessingEvent) -> F + Send + Sync>,
    ) -> ClientResult<ResultOfSendMessage> {
        let endpoint = Endpoint::resolve(context.env.clone(), endpoint_address).await?;
        let shard_block_id =
            match find_last_shard_block(&context, &self.dst, Some(endpoint.clone())).await {
                Ok(block) => block.to_string(),
                Err(err) => {
                    if let Some(callback) = &callback {
                        callback(ProcessingEvent::FetchFirstBlockFailed { error: err.clone() })
                            .await;
                    }
                    return Err(Error::fetch_first_block_failed(err, &self.id));
                }
            };

        if let Some(callback) = &callback {
            callback(ProcessingEvent::WillSend {
                shard_block_id: shard_block_id.clone(),
                message_id: self.id.to_string(),
                message: self.serialized.clone(),
            })
            .await;
        }

        // Send
        let send_error = context
            .get_server_link()?
            .send_message(&hex_decode(&self.id)?, &self.body, Some(endpoint))
            .await
            .add_network_url_from_context(&context)
            .await?;

        if let Some(callback) = &callback {
            callback(match send_error {
                None => ProcessingEvent::DidSend {
                    shard_block_id: shard_block_id.clone(),
                    message_id: self.id.clone(),
                    message: self.serialized.clone(),
                },
                Some(error) => ProcessingEvent::SendFailed {
                    shard_block_id: shard_block_id.clone(),
                    message_id: self.id.clone(),
                    message: self.serialized.clone(),
                    error: Error::send_message_failed(error, &self.id, &shard_block_id),
                },
            })
            .await;
        }
        Ok(ResultOfSendMessage { shard_block_id })
    }
}

pub async fn send_message<F: futures::Future<Output = ()> + Send>(
    context: Arc<ClientContext>,
    params: ParamsOfSendMessage,
    callback: impl Fn(ProcessingEvent) -> F + Send + Sync + Clone,
) -> ClientResult<ResultOfSendMessage> {
    let message = SendingMessage::new(&context, &params.message, params.abi.as_ref()).await?;

    // Fetch current shard block
    if params.send_events {
        callback(ProcessingEvent::WillFetchFirstBlock {}).await;
    }
    let mut available_addresses = context.get_server_link()?.get_endpoint_addresses().await;
    available_addresses.shuffle(&mut rand::thread_rng());
    let mut last_result = None;

    for selected_addresses in
        available_addresses.chunks(context.config.network.sending_endpoint_count as usize)
    {
        let mut futures = vec![];
        for address in selected_addresses {
            let context = context.clone();
            let callback = if params.send_events {
                Some(callback.clone())
            } else {
                None
            };
            let message = message.clone();
            futures.push(Box::pin(async move {
                message.send_to_endpoint(context, &address, callback).await
            }));
        }
        match futures::future::select_ok(futures).await {
            Ok((result, _)) => {
                return Ok(result);
            }
            Err(err) => {
                last_result = Some(Err(err));
            }
        }
    }
    last_result.unwrap_or_else(|| Err(Error::block_not_found("no endpoints".to_string())))
}
