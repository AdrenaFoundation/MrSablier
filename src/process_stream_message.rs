use {
    crate::{
        evaluate_and_run_automated_orders::evaluate_and_run_automated_orders,
        generate_accounts_filter_map,
        update_indexes::{update_indexed_limit_order_books, update_indexed_positions},
        IndexedCustodiesThreadSafe, IndexedLimitOrderBooksThreadSafe, IndexedPositionsThreadSafe, PriorityFeesThreadSafe,
    },
    adrena_abi::{
        types::{Cortex, Position},
        LimitOrderBook, Pool,
    },
    anchor_client::{Client, Cluster},
    futures::{channel::mpsc::SendError, Sink, SinkExt},
    solana_sdk::{pubkey::Pubkey, signature::Keypair},
    std::sync::Arc,
    yellowstone_grpc_proto::geyser::{subscribe_update::UpdateOneof, SubscribeRequest, SubscribeRequestPing, SubscribeUpdate},
};

pub enum LimitOrderBookUpdate {
    Created(LimitOrderBook),
    Modified(LimitOrderBook),
    Closed,
}

pub enum PositionUpdate {
    Created(Position),
    Modified(Position),
    Closed,
}

pub async fn process_stream_message<S>(
    message: Result<SubscribeUpdate, backoff::Error<anyhow::Error>>,
    indexed_positions: &IndexedPositionsThreadSafe,
    indexed_custodies: &IndexedCustodiesThreadSafe,
    indexed_limit_order_books: &IndexedLimitOrderBooksThreadSafe,
    payer: &Arc<Keypair>,
    endpoint: &str,
    cortex: &Cortex,
    pool: &Pool,
    subscribe_tx: &mut S,
    priority_fees: &PriorityFeesThreadSafe,
) -> Result<(), backoff::Error<anyhow::Error>>
where
    S: Sink<SubscribeRequest, Error = SendError> + Unpin,
{
    let mut subscriptions_update_required = false;
    let program = Client::new(Cluster::Custom(endpoint.to_string(), endpoint.to_string()), Arc::clone(payer))
        .program(adrena_abi::ID)
        .map_err(|e| backoff::Error::transient(e.into()))?;

    match message {
        Ok(msg) => {
            match msg.update_oneof {
                Some(UpdateOneof::Account(sua)) => {
                    let account = sua.account.expect("Account should be defined");
                    let account_key = Pubkey::try_from(account.pubkey).expect("valid pubkey");
                    let account_data = account.data.to_vec();
                    // Each loop iteration we check if we need to update the subscription request based on what previously happened

                    if msg.filters.contains(&"price_feeds".to_owned()) {
                        // let start = std::time::Instant::now();
                        evaluate_and_run_automated_orders(
                            &account_key,
                            &account_data,
                            indexed_positions,
                            indexed_custodies,
                            indexed_limit_order_books,
                            payer,
                            endpoint,
                            cortex,
                            pool,
                            priority_fees,
                        )
                        .await?;
                        // log::info!(
                        //     "   <*> evaluate_and_run_automated_orders took {:?}",
                        //     start.elapsed()
                        // );
                        // No subscriptions update needed here as this will trigger the update in the next cycle for position change (and update filters there)
                    }

                    if msg.filters.contains(&"positions_create_update".to_owned()) {
                        // Updates the indexed positions map
                        let update = update_indexed_positions(&account_key, &account_data, indexed_positions).await?;

                        // Update the indexed custodies map and the subscriptions request if a new position was created
                        match update {
                            PositionUpdate::Created(new_position) => {
                                log::info!("(pcu) New position created: {:#?}", account_key);
                                // If the new position's custody is not yet in the indexed custodies map, fetch it from the RPC
                                if !indexed_custodies.read().await.contains_key(&new_position.custody) {
                                    let custody = program
                                        .account::<adrena_abi::types::Custody>(new_position.custody)
                                        .await
                                        .map_err(|e| backoff::Error::transient(e.into()))?;

                                    // Update the indexed custodies map
                                    indexed_custodies.write().await.insert(new_position.custody, custody);
                                    log::info!("(pcu) Fetched new custody {:#?} from RPC", new_position.custody);
                                }

                                // We need to update the subscriptions request to include the new position (and maybe the new custody's price update v2 account)
                                subscriptions_update_required = true;
                            }
                            PositionUpdate::Modified(_position) => {
                                log::info!("(pcu) Position modified: {:#?}", account_key);
                            }
                            PositionUpdate::Closed => {
                                log::info!("(pcu) Position closed: {:#?}", account_key);
                            }
                        }
                    }
                    /* Else if is important as we only want to end up here if the message is not about a  positions_create_update */
                    else if msg.filters.contains(&"positions_close".to_owned()) {
                        // Updates the indexed positions map
                        let update = update_indexed_positions(&account_key, &account_data, indexed_positions).await?;

                        match update {
                            PositionUpdate::Created(_) => {
                                panic!("New position created in positions_close filter");
                            }
                            PositionUpdate::Modified(_) => {
                                panic!("New position created in positions_close filter");
                            }
                            PositionUpdate::Closed => {
                                log::info!("(pc) Position closed: {:#?}", account_key);
                                // We need to update the subscriptions request to remove the closed position
                                subscriptions_update_required = true;
                            }
                        }
                    } else if msg.filters.contains(&"limit_order_books_create_update".to_owned()) {
                        let update = update_indexed_limit_order_books(
                            &account_key,
                            &account_data,
                            account.lamports,
                            indexed_limit_order_books,
                        )
                        .await?;

                        match update {
                            LimitOrderBookUpdate::Created(_new_limit_order_book) => {
                                log::info!("(lobcu) New limit order book created: {:#?}", account_key);
                            }
                            LimitOrderBookUpdate::Modified(_limit_order_book) => {
                                log::info!("(lobcu) LimitOrderBook modified: {:#?}", account_key);
                            }
                            LimitOrderBookUpdate::Closed => {
                                log::info!("(lobcu) LimitOrderBook closed: {:#?}", account_key);

                                // We need to update the subscriptions request to remove the closed limit order book
                                subscriptions_update_required = true;
                            }
                        }
                    } else if msg.filters.contains(&"limit_order_books_close".to_owned()) {
                        let update = update_indexed_limit_order_books(
                            &account_key,
                            &account_data,
                            account.lamports,
                            indexed_limit_order_books,
                        )
                        .await?;

                        match update {
                            LimitOrderBookUpdate::Created(_) => {
                                panic!("New limit order book created in limit_order_books_close filter");
                            }
                            LimitOrderBookUpdate::Modified(_) => {
                                panic!("New limit order book created in limit_order_books_close filter");
                            }
                            LimitOrderBookUpdate::Closed => {
                                log::info!("(lobc) LimitOrderBook closed: {:#?}", account_key);
                                // We need to update the subscriptions request to remove the closed limit order book
                                subscriptions_update_required = true;
                            }
                        }
                    }
                }
                Some(UpdateOneof::Ping(_)) => {
                    log::debug!("  <> Received ping message");
                    // This is necessary to keep load balancers that expect client pings alive. If your load balancer doesn't
                    // require periodic client pings then this is unnecessary
                    subscribe_tx
                        .send(SubscribeRequest {
                            ping: Some(SubscribeRequestPing { id: 1 }),
                            ..Default::default()
                        })
                        .await
                        .map_err(|e| backoff::Error::transient(e.into()))?;
                }
                _ => {}
            }
        }
        Err(error) => {
            log::error!("error: {error:?}");
            return Err(error);
        }
    }

    // Update the subscriptions request if needed
    if subscriptions_update_required {
        log::info!("  <> Update subscriptions request");
        let accounts_filter_map =
            generate_accounts_filter_map(indexed_custodies, indexed_positions, indexed_limit_order_books).await;
        let request = SubscribeRequest {
            ping: None, //Some(SubscribeRequestPing { id: 1 }),
            accounts: accounts_filter_map,
            ..Default::default()
        };
        subscribe_tx
            .send(request)
            .await
            .map_err(|e| backoff::Error::transient(e.into()))?;
    }
    Ok(())
}
