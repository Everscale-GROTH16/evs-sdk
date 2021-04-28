use tokio::sync::Mutex;

use crate::abi::{CallSet, DeploySet, ParamsOfEncodeMessage, Signer};
use crate::error::ClientError;
use crate::processing::ParamsOfProcessMessage;
use crate::tests::{TestClient, TestFetchQueue, HELLO};

use super::*;
use crate::ClientConfig;
use std::collections::HashSet;
use std::sync::Arc;

#[tokio::test(core_threads = 2)]
async fn batch_query() {
    let client = TestClient::new();

    let batch: ResultOfBatchQuery = client
        .request_async(
            "net.batch_query",
            ParamsOfBatchQuery {
                operations: vec![
                    ParamsOfQueryOperation::QueryCollection(ParamsOfQueryCollection {
                        collection: "blocks_signatures".to_owned(),
                        filter: None,
                        result: "id".to_owned(),
                        limit: Some(1),
                        order: None,
                    }),
                    ParamsOfQueryOperation::AggregateCollection(ParamsOfAggregateCollection {
                        collection: "accounts".to_owned(),
                        filter: None,
                        fields: Some(vec![FieldAggregation {
                            field: "".into(),
                            aggregation_fn: AggregationFn::COUNT,
                        }]),
                    }),
                    ParamsOfQueryOperation::WaitForCollection(ParamsOfWaitForCollection {
                        collection: "transactions".to_owned(),
                        filter: Some(json!({
                            "now": { "gt": 20 }
                        })),
                        result: "id now".to_owned(),
                        timeout: None,
                    }),
                ],
            },
        )
        .await
        .unwrap();

    assert_eq!(batch.results.len(), 3);
}

#[tokio::test(core_threads = 2)]
async fn query() {
    let client = TestClient::new();

    let info: ResultOfQuery = client
        .request_async(
            "net.query",
            ParamsOfQuery {
                query: "query{info{version}}".to_owned(),
                variables: None,
            },
        )
        .await
        .unwrap();

    let version = info.result["data"]["info"]["version"].as_str().unwrap();
    assert_eq!(version.split(".").count(), 3);
}

#[tokio::test(core_threads = 2)]
async fn block_signatures() {
    let client = TestClient::new();

    let _: ResultOfQueryCollection = client
        .request_async(
            "net.query_collection",
            ParamsOfQueryCollection {
                collection: "blocks_signatures".to_owned(),
                filter: Some(json!({})),
                result: "id".to_owned(),
                limit: Some(1),
                order: None,
            },
        )
        .await
        .unwrap();
}

#[tokio::test(core_threads = 2)]
async fn all_accounts() {
    let client = TestClient::new();

    let accounts: ResultOfQueryCollection = client
        .request_async(
            "net.query_collection",
            ParamsOfQueryCollection {
                collection: "accounts".to_owned(),
                filter: Some(json!({})),
                result: "id balance".to_owned(),
                limit: None,
                order: None,
            },
        )
        .await
        .unwrap();

    assert!(accounts.result.len() > 0);
}

#[tokio::test(core_threads = 2)]
async fn aggregates() {
    let client = TestClient::new();

    let result: ResultOfAggregateCollection = client
        .request_async(
            "net.aggregate_collection",
            ParamsOfAggregateCollection {
                collection: "accounts".to_owned(),
                filter: Some(json!({})),
                fields: Some(vec![FieldAggregation {
                    field: "".into(),
                    aggregation_fn: AggregationFn::COUNT,
                }]),
            },
        )
        .await
        .unwrap();

    let count = u32::from_str_radix(result.values[0].as_str().unwrap(), 10).unwrap();
    assert!(count > 0);
}

#[tokio::test(core_threads = 2)]
async fn ranges() {
    let client = TestClient::new();

    let accounts: ResultOfQueryCollection = client
        .request_async(
            "net.query_collection",
            ParamsOfQueryCollection {
                collection: "messages".to_owned(),
                filter: Some(json!({
                    "created_at": { "gt": 1562342740 }
                })),
                result: "body created_at".to_owned(),
                limit: None,
                order: None,
            },
        )
        .await
        .unwrap();

    assert!(accounts.result[0]["created_at"].as_u64().unwrap() > 1562342740);
}

#[tokio::test(core_threads = 2)]
async fn wait_for() {
    let now = ton_sdk::Contract::now();
    let request = tokio::spawn(async move {
        let client = TestClient::new();
        let transactions: ResultOfWaitForCollection = client
            .request_async(
                "net.wait_for_collection",
                ParamsOfWaitForCollection {
                    collection: "transactions".to_owned(),
                    filter: Some(json!({
                        "now": { "gt": now }
                    })),
                    result: "id now".to_owned(),
                    timeout: None,
                },
            )
            .await
            .unwrap();
        assert!(transactions.result["now"].as_u64().unwrap() > now as u64);
    });

    let client = TestClient::new();

    client
        .get_tokens_from_giver_async(&client.giver_address().await, None)
        .await;

    request.await.unwrap();
}

#[tokio::test(core_threads = 2)]
async fn message_sending_addresses() {
    let client = ClientContext::new(ClientConfig {
        network: NetworkConfig {
            endpoints: Some(vec![
                "a".into(),
                "b".into(),
                "c".into(),
                "d".into(),
                "e".into(),
                "f".into(),
                "g".into(),
                "h".into(),
            ]),
            ..Default::default()
        },
        ..Default::default()
    })
    .unwrap();
    let link = client.get_server_link().unwrap();
    link.update_stat(
        &vec!["a".to_string(), "e".to_string()],
        EndpointStat::MessageUndelivered,
    )
    .await;
    let bad: HashSet<_> = vec!["a".to_string(), "e".to_string()]
        .iter()
        .cloned()
        .collect();
    for _ in 0..100 {
        let addresses = link.get_addresses_for_sending().await;
        let tail: HashSet<_> = addresses[addresses.len() - 2..].iter().cloned().collect();
        assert_eq!(tail, bad);
    }
    link.update_stat(
        &vec!["a".to_string(), "e".to_string()],
        EndpointStat::MessageDelivered,
    )
    .await;
    let mut a_good = false;
    let mut e_good = false;
    for _ in 0..100 {
        let addresses = link.get_addresses_for_sending().await;
        let tail: HashSet<_> = addresses[addresses.len() - 2..].iter().cloned().collect();
        if !tail.contains("a") {
            a_good = true;
        }
        if !tail.contains("e") {
            e_good = true;
        }
    }
    assert!(a_good && e_good)
}

#[tokio::test(core_threads = 2)]
async fn subscribe_for_transactions_with_addresses() {
    let client = TestClient::new_with_config(json!({
        "network": {
            "endpoints": TestClient::endpoints(),
        }
    }));
    let subscription_client = TestClient::new();
    let keys = subscription_client.generate_sign_keys();
    let deploy_params = ParamsOfEncodeMessage {
        abi: TestClient::abi(HELLO, None),
        deploy_set: Some(DeploySet {
            tvc: TestClient::tvc(HELLO, None),
            ..Default::default()
        }),
        signer: Signer::Keys { keys: keys.clone() },
        processing_try_index: None,
        address: None,
        call_set: CallSet::some_with_function("constructor"),
    };
    let msg = subscription_client
        .encode_message(deploy_params.clone())
        .await
        .unwrap();
    let transactions = std::sync::Arc::new(Mutex::new(vec![]));
    let transactions_copy1 = transactions.clone();
    let transactions_copy2 = transactions.clone();
    let notifications = std::sync::Arc::new(Mutex::new(vec![]));
    let notifications_copy1 = notifications.clone();
    let notifications_copy2 = notifications.clone();
    let address1 = msg.address.clone();
    let address2 = msg.address.clone();

    let callback1 = move |result: serde_json::Value, response_type: SubscriptionResponseType| {
        let result = match response_type {
            SubscriptionResponseType::Ok => {
                Ok(serde_json::from_value::<ResultOfSubscription>(result).unwrap())
            }
            SubscriptionResponseType::Error => {
                Err(serde_json::from_value::<ClientError>(result).unwrap())
            }
        };
        let address1 = address1.clone();
        let transactions_copy = transactions_copy1.clone();
        let notifications_copy = notifications_copy1.clone();
        async move {
            match result {
                Ok(result) => {
                    assert_eq!(result.result["account_addr"], address1);
                    transactions_copy.lock().await.push(result.result);
                }
                Err(err) => {
                    println!(">>> {}", err);
                    notifications_copy.lock().await.push(err);
                }
            }
        }
    };

    let handle1: ResultOfSubscribeCollection = subscription_client.request_async_callback(
            "net.subscribe_collection",
            ParamsOfSubscribeCollection {
                collection: "transactions".to_owned(),
                filter: Some(json!({
                    "account_addr": { "eq": msg.address.clone() },
                    "status": { "eq": ton_sdk::json_helper::transaction_status_to_u8(ton_block::TransactionProcessingStatus::Finalized) }
                })),
                result: "id account_addr".to_owned(),
            },
            callback1
        ).await.unwrap();

    // send grams to create first transaction
    client.get_tokens_from_giver_async(&msg.address, None).await;

    // give some time for subscription to receive all data
    std::thread::sleep(std::time::Duration::from_millis(1000));

    {
        // check that transaction is received
        let transactions = transactions.lock().await;
        assert_eq!(transactions.len(), 1);
        // and no error notifications
        let notifications = notifications.lock().await;
        assert_eq!(notifications.len(), 0);
    }

    // suspend subscription
    let _: () = subscription_client
        .request_async("net.suspend", ())
        .await
        .unwrap();

    // deploy to create second transaction
    client
        .net_process_message(
            ParamsOfProcessMessage {
                message_encode_params: deploy_params,
                send_events: false,
            },
            TestClient::default_callback,
        )
        .await
        .unwrap();

    // create second subscription while network is suspended
    let callback2 = move |result: serde_json::Value, response_type: SubscriptionResponseType| {
        let result = match response_type {
            SubscriptionResponseType::Ok => {
                Ok(serde_json::from_value::<ResultOfSubscription>(result).unwrap())
            }
            SubscriptionResponseType::Error => {
                Err(serde_json::from_value::<ClientError>(result).unwrap())
            }
        };
        let transactions_copy = transactions_copy2.clone();
        let notifications_copy = notifications_copy2.clone();
        let address2 = address2.clone();
        async move {
            match result {
                Ok(result) => {
                    assert_eq!(result.result["account_addr"], address2);
                    transactions_copy.lock().await.push(result.result);
                }
                Err(err) => {
                    println!(">>> {}", err);
                    notifications_copy.lock().await.push(err);
                }
            }
        }
    };

    let handle2: ResultOfSubscribeCollection = subscription_client.request_async_callback(
            "net.subscribe_collection",
            ParamsOfSubscribeCollection {
                collection: "transactions".to_owned(),
                filter: Some(json!({
                    "account_addr": { "eq": msg.address.clone() },
                    "status": { "eq": ton_sdk::json_helper::transaction_status_to_u8(ton_block::TransactionProcessingStatus::Finalized) }
                })),
                result: "id account_addr".to_owned(),
            },
            callback2
        ).await.unwrap();

    // give some time for subscription to receive all data
    std::thread::sleep(std::time::Duration::from_millis(500));
    {
        // check that second transaction is not received when subscription suspended
        let transactions = transactions.lock().await;
        assert_eq!(transactions.len(), 1);
        // and both subscriptions received notification about suspend
        let notifications = notifications.lock().await;
        assert_eq!(notifications.len(), 2);
        assert_eq!(notifications[0], Error::network_module_suspended());
        assert_eq!(notifications[1], Error::network_module_suspended());
    }

    // resume subscription
    let _: () = subscription_client
        .request_async("net.resume", ())
        .await
        .unwrap();

    // run contract function to create third transaction
    subscription_client
        .net_process_message(
            ParamsOfProcessMessage {
                message_encode_params: ParamsOfEncodeMessage {
                    abi: TestClient::abi(HELLO, None),
                    deploy_set: None,
                    signer: Signer::Keys { keys },
                    processing_try_index: None,
                    address: Some(msg.address),
                    call_set: CallSet::some_with_function("touch"),
                },
                send_events: false,
            },
            TestClient::default_callback,
        )
        .await
        .unwrap();

    // give some time for subscription to receive all data
    std::thread::sleep(std::time::Duration::from_millis(5000));

    // check that third transaction is now received after resume
    let transactions = transactions.lock().await;
    assert_eq!(transactions.len(), 3);
    assert_ne!(transactions[0]["id"], transactions[2]["id"]);
    // and both subscriptions received notification about resume
    let notifications = notifications.lock().await;
    assert_eq!(notifications.len(), 4);
    assert_eq!(notifications[2], Error::network_module_resumed());
    assert_eq!(notifications[3], Error::network_module_resumed());

    let _: () = subscription_client
        .request_async("net.unsubscribe", handle1)
        .await
        .unwrap();
    let _: () = subscription_client
        .request_async("net.unsubscribe", handle2)
        .await
        .unwrap();
}

#[tokio::test(core_threads = 2)]
async fn subscribe_for_messages() {
    let messages = std::sync::Arc::new(Mutex::new(Vec::new()));
    let messages_copy = messages.clone();

    let callback = move |result: serde_json::Value, response_type: SubscriptionResponseType| {
        let result = match response_type {
            SubscriptionResponseType::Ok => {
                Ok(serde_json::from_value::<ResultOfSubscription>(result).unwrap())
            }
            SubscriptionResponseType::Error => {
                Err(serde_json::from_value::<ClientError>(result).unwrap())
            }
        }
        .unwrap();
        let messages_copy = messages_copy.clone();
        async move {
            messages_copy.lock().await.push(result.result);
        }
    };

    let client = TestClient::new();

    let handle: ResultOfSubscribeCollection = client
        .request_async_callback(
            "net.subscribe_collection",
            ParamsOfSubscribeCollection {
                collection: "messages".to_owned(),
                filter: Some(json!({
                    "dst": { "eq": "1" }
                })),
                result: "id".to_owned(),
            },
            callback,
        )
        .await
        .unwrap();

    client
        .get_tokens_from_giver_async(&client.giver_address().await, None)
        .await;

    assert_eq!(messages.lock().await.len(), 0);

    let _: () = client
        .request_async("net.unsubscribe", handle)
        .await
        .unwrap();
}

#[tokio::test(core_threads = 2)]
async fn find_last_shard_block() {
    let client = TestClient::new();

    let block: ResultOfFindLastShardBlock = client
        .request_async(
            "net.find_last_shard_block",
            ParamsOfFindLastShardBlock {
                address: client.giver_address().await,
            },
        )
        .await
        .unwrap();

    println!("{}", block.block_id);
}

// #[tokio::test(core_threads = 2)]
// async fn test_endpoints() {
//     let client = TestClient::new_with_config(json!({
//         "network": {
//             "endpoints": ["cinet.tonlabs.io", "cinet2.tonlabs.io/"],
//         }
//     }));

//     let endpoints: EndpointsSet = client
//         .request_async("net.fetch_endpoints", ())
//         .await
//         .unwrap();

//     let _: () = client
//         .request_async("net.set_endpoints", endpoints)
//         .await
//         .unwrap();
// }

#[tokio::test(core_threads = 2)]
async fn test_wait_resume() {
    let client = std::sync::Arc::new(TestClient::new());
    let client_copy = client.clone();

    let _: () = client.request_async("net.suspend", ()).await.unwrap();

    let start = std::time::Instant::now();

    let duration = tokio::spawn(async move {
        client_copy
            .fetch_account(&client_copy.giver_address().await)
            .await;

        start.elapsed().as_millis()
    });

    let timeout = 5000;
    tokio::time::delay_for(tokio::time::Duration::from_millis(timeout)).await;

    let _: () = client.request_async("net.resume", ()).await.unwrap();

    assert!(duration.await.unwrap() > timeout as u128);
}

#[tokio::test(core_threads = 2)]
async fn test_query_counterparties() {
    if TestClient::node_se() {
        return;
    }

    let client = TestClient::new();

    let account = client.giver_address().await;

    let counterparties1: ResultOfQueryCollection = client
        .request_async(
            "net.query_counterparties",
            ParamsOfQueryCounterparties {
                account: account.clone(),
                first: Some(5),
                after: None,
                result: "counterparty last_message_id cursor".to_owned(),
            },
        )
        .await
        .unwrap();

    assert!(counterparties1.result.len() <= 5);

    if counterparties1.result.len() == 5 {
        let counterparties2: ResultOfQueryCollection = client
            .request_async(
                "net.query_counterparties",
                ParamsOfQueryCounterparties {
                    account: account.clone(),
                    first: Some(5),
                    after: Some(
                        counterparties1.result[4]["cursor"]
                            .as_str()
                            .unwrap()
                            .to_owned(),
                    ),
                    result: "counterparty last_message_id cursor".to_owned(),
                },
            )
            .await
            .unwrap();

        assert_ne!(counterparties1.result, counterparties2.result);
    }
}

fn r_info(time: u64, block_time: u64) -> String {
    json!({
        "data": {
            "info": {
                "version": "0.37.0",
                "time": time,
                "lastBlockTime": block_time,
            }
        }
    })
    .to_string()
}

fn r_blocks(id: &str) -> String {
    json!({
        "data": {
            "blocks": [{"id": id.to_string()}],
        }
    })
    .to_string()
}

async fn query_block_id(client: &Arc<ClientContext>) -> String {
    crate::net::query_collection(
        client.clone(),
        ParamsOfQueryCollection {
            collection: "blocks".to_string(),
            result: "id".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .result[0]["id"]
        .as_str()
        .unwrap()
        .to_string()
}

async fn get_query_url(client: &Arc<ClientContext>) -> String {
    let mut url = client
        .get_server_link()
        .unwrap()
        .get_query_endpoint()
        .await
        .unwrap()
        .query_url
        .clone();
    if let Some(stripped) = url.strip_prefix("http://") {
        url = stripped.to_string();
    }
    if let Some(stripped) = url.strip_prefix("https://") {
        url = stripped.to_string();
    }
    if let Some(stripped) = url.strip_suffix("/graphql") {
        url = stripped.to_string();
    }
    url
}

#[tokio::test(core_threads = 2)]
async fn retry_query_on_network_errors() {
    let client = Arc::new(
        ClientContext::new(ClientConfig {
            network: NetworkConfig {
                endpoints: Some(vec!["a".into()]),
                ..Default::default()
            },
            ..Default::default()
        })
        .unwrap(),
    );

    let now = client.env.now_ms();
    TestFetchQueue::new()
        .url("a")
        .ok(&r_info(now, now - 1000))
        .repeat(2)
        .network_err()
        .ok(&r_info(now, now - 1000))
        .ok(&r_blocks("1"))
        .repeat(5)
        .network_err()
        .ok(&r_info(now, now - 1000))
        .ok(&r_blocks("2"))
        .reset_client(&client)
        .await;
    assert_eq!(query_block_id(&client).await, "1");
    assert_eq!(query_block_id(&client).await, "2");
}

#[tokio::test(core_threads = 2)]
async fn querying_endpoint_selection() {
    let client = Arc::new(
        ClientContext::new(ClientConfig {
            network: NetworkConfig {
                endpoints: Some(vec!["a".into(), "b".into()]),
                network_retries_count: 3,
                max_latency: 1000,
                ..Default::default()
            },
            ..Default::default()
        })
        .unwrap(),
    );

    // Check for the fastest
    let now = client.env.now_ms();
    TestFetchQueue::new()
        .url("a")
        .delay(200)
        .ok(&r_info(now, now - 500))
        .url("b")
        .delay(100)
        .ok(&r_info(now, now - 500))
        .reset_client(&client)
        .await;
    assert_eq!(get_query_url(&client).await, "b");

    println!("\nSkip endpoint with bad latency\n");
    let now = client.env.now_ms();
    TestFetchQueue::new()
        .url("a")
        .delay(100)
        .ok(&r_info(now, now - 1500))
        .url("b")
        .delay(200)
        .ok(&r_info(now, now - 500))
        .reset_client(&client)
        .await;
    assert_eq!(get_query_url(&client).await, "b");

    println!("\nSelect when all have bad latency\n");
    let now = client.env.now_ms();
    TestFetchQueue::new()
        .url("a")
        .delay(200)
        .ok(&r_info(now, now - 1500)) // Slower but better latency
        .url("b")
        .delay(100)
        .ok(&r_info(now, now - 2000)) // Faster but worse latency
        .reset_client(&client)
        .await;
    assert_eq!(get_query_url(&client).await, "a");

    println!("\nFailed Network\n");
    let client = Arc::new(
        ClientContext::new(ClientConfig {
            network: NetworkConfig {
                endpoints: Some(vec!["a".into()]),
                network_retries_count: 2,
                max_latency: 1000,
                ..Default::default()
            },
            ..Default::default()
        })
        .unwrap(),
    );
    TestFetchQueue::new()
        .url("a")
        .repeat(3)
        .network_err()
        .reset_client(&client)
        .await;
    let result = crate::net::query_collection(
        client.clone(),
        ParamsOfQueryCollection {
            collection: "blocks".to_string(),
            result: "id id".to_string(),
            ..Default::default()
        },
    )
    .await;

    assert_eq!(
        match &result {
            Ok(_) => "ok",
            Err(e) => &e.message,
        },
        "Query failed: Can not send http request: Network error"
    );
}

#[tokio::test(core_threads = 2)]
async fn latency_detection_with_queries() {
    let client = Arc::new(
        ClientContext::new(ClientConfig {
            network: NetworkConfig {
                endpoints: Some(vec!["a".into(), "b".into()]),
                network_retries_count: 3,
                max_latency: 600,
                latency_detection_frequency: 100,
                ..Default::default()
            },
            ..Default::default()
        })
        .unwrap(),
    );

    // Check for the fastest
    let now = client.env.now_ms();
    TestFetchQueue::new()
        .url("a")
        .delay(10)
        .ok(&r_info(now, now - 500)) // election, winner
        .url("b")
        .delay(20)
        .ok(&r_info(now, now - 500)) // election, looser
        .url("a")
        .delay(200)
        .ok(&r_blocks("1")) // query
        .ok(&json!({
            "data": {
                "q1": [{
                    "id": "2",
                }],
                "q2": {
                    "version": "0.37.0",
                    "time": 1000,
                    "lastBlockTime": 100,
                },
            }
        })
        .to_string()) // query with latency checking, returns bad latency
        .url("a")
        .delay(20)
        .ok(&r_info(now, now - 500)) // election, looser
        .url("b")
        .delay(10)
        .ok(&r_info(now, now - 500)) // election, winner
        .url("b")
        .ok(&r_blocks("2")) // retry query
        .reset_client(&client)
        .await;

    assert_eq!(get_query_url(&client).await, "a");
    assert_eq!(query_block_id(&client).await, "1");
    assert_eq!(query_block_id(&client).await, "2");
    assert_eq!(TestFetchQueue::get_len(&client).await, 0);
    assert_eq!(get_query_url(&client).await, "b");
}
