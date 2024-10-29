use {
    crate::{
        evaluate_and_run_automated_orders::evaluate_and_run_automated_orders,
        generate_accounts_filter_map,
        handlers::{self, cleanup_position_stop_loss, cleanup_position_take_profit},
        update_indexes::update_indexed_positions,
        IndexedCustodiesThreadSafe, IndexedPositionsThreadSafe,
    },
    adrena_abi::types::{Cortex, Position},
    anchor_client::{Client, Cluster},
    futures::{channel::mpsc::SendError, Sink, SinkExt},
    solana_sdk::{pubkey::Pubkey, signature::Keypair},
    std::sync::Arc,
    yellowstone_grpc_proto::geyser::{
        subscribe_update::UpdateOneof, SubscribeRequest, SubscribeUpdate,
    },
};

pub enum PositionUpdate {
    Created(Position),
    Modified(Position),
    PendingCleanupAndClose(Position),
    Closed,
}

pub async fn process_stream_message<S>(
    message: Result<SubscribeUpdate, backoff::Error<anyhow::Error>>,
    indexed_positions: &IndexedPositionsThreadSafe,
    indexed_custodies: &IndexedCustodiesThreadSafe,
    payer: &Arc<Keypair>,
    endpoint: &str,
    cortex: &Cortex,
    subscribe_tx: &mut S,
    median_priority_fee: u64,
) -> Result<(), backoff::Error<anyhow::Error>>
where
    S: Sink<SubscribeRequest, Error = SendError> + Unpin,
{
    let mut subscriptions_update_required = false;

    match message {
        Ok(msg) => {
            if let Some(UpdateOneof::Account(sua)) = msg.update_oneof {
                let account = sua.account.expect("Account should be defined");
                let account_key = Pubkey::try_from(account.pubkey).expect("valid pubkey");
                let account_data = account.data.to_vec();
                // Each loop iteration we check if we need to update the subscription request based on what previously happened

                if msg.filters.contains(&"price_feeds".to_owned()) {
                    evaluate_and_run_automated_orders(
                        &account_key,
                        &account_data,
                        indexed_positions,
                        indexed_custodies,
                        payer,
                        endpoint,
                        cortex,
                        median_priority_fee,
                    )
                    .await?;
                    // No subscriptions update needed here as this will trigger the update in the next cycle for position change (and update filters there)
                }

                if msg.filters.contains(&"positions_create_update".to_owned()) {
                    // Updates the indexed positions map
                    let update =
                        update_indexed_positions(&account_key, &account_data, indexed_positions)
                            .await?;

                    // Update the indexed custodies map and the subscriptions request if a new position was created
                    match update {
                        PositionUpdate::Created(new_position) => {
                            log::info!("(pcu) New position created: {:#?}", account_key);
                            // If the new position's custody is not yet in the indexed custodies map, fetch it from the RPC
                            if !indexed_custodies
                                .read()
                                .await
                                .contains_key(&new_position.custody)
                            {
                                let client = Client::new(
                                    Cluster::Custom(endpoint.to_string(), endpoint.to_string()),
                                    Arc::clone(payer),
                                );
                                let program = client
                                    .program(adrena_abi::ID)
                                    .map_err(|e| backoff::Error::transient(e.into()))?;

                                let custody = program
                                    .account::<adrena_abi::types::Custody>(new_position.custody)
                                    .await
                                    .map_err(|e| backoff::Error::transient(e.into()))?;

                                // Update the indexed custodies map
                                indexed_custodies
                                    .write()
                                    .await
                                    .insert(new_position.custody, custody);
                                log::info!(
                                    "(pcu) Fetched new custody {:#?} from RPC",
                                    new_position.custody
                                );
                            }

                            // We need to update the subscriptions request to include the new position (and maybe the new custody's price update v2 account)
                            subscriptions_update_required = true;
                        }
                        PositionUpdate::PendingCleanupAndClose(position) => {
                            log::info!(
                                "(pcu) Position pending cleanup and close: {:#?}",
                                account_key
                            );
                            // Do the cleanup and close for the position in stasis
                            let client = Client::new(
                                Cluster::Custom(endpoint.to_string(), endpoint.to_string()),
                                Arc::clone(payer),
                            );
                            cleanup_position(&account_key, &position, &client, median_priority_fee)
                                .await?;
                        }
                        PositionUpdate::Modified(_position) => {
                            log::info!("(pcu) Position modified: {:#?}", account_key);
                        }
                        PositionUpdate::Closed => {
                            log::info!("(pcu) Position closed: {:#?}", account_key);
                        }
                    }
                }
                /* Else if is important as we only want to end up here if the message is not about a  positions_create_update*/
                else if msg.filters.contains(&"positions_close".to_owned()) {
                    // Updates the indexed positions map
                    let update =
                        update_indexed_positions(&account_key, &account_data, indexed_positions)
                            .await?;
                    match update {
                        PositionUpdate::Created(_) => {
                            panic!("New position created in positions_close filter");
                        }
                        PositionUpdate::Modified(_) => {
                            panic!("New position created in positions_close filter");
                        }
                        PositionUpdate::PendingCleanupAndClose(_) => {
                            panic!("New position created in positions_close filter");
                        }
                        PositionUpdate::Closed => {
                            log::info!("(pc) Position closed: {:#?}", account_key);
                            // We need to update the subscriptions request to remove the closed position
                            subscriptions_update_required = true;
                        }
                    }
                }
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
            generate_accounts_filter_map(indexed_custodies, indexed_positions).await;
        let request = SubscribeRequest {
            // ping: None,
            accounts: accounts_filter_map,
            // commitment: commitment.map(|c| c.into()),
            ..Default::default()
        };
        subscribe_tx
            .send(request)
            .await
            .map_err(|e| backoff::Error::transient(e.into()))?;
    }
    Ok(())
}

pub async fn cleanup_position(
    position_key: &Pubkey,
    position: &adrena_abi::types::Position,
    client: &Client<Arc<Keypair>>,
    median_priority_fee: u64,
) -> Result<(), backoff::Error<anyhow::Error>> {
    if position.take_profit_is_set() && position.stop_loss_is_set() {
        handlers::cleanup_position_sl_tp::cleanup_position(
            position_key,
            position,
            client,
            median_priority_fee,
        )
        .await?;
    } else if position.stop_loss_is_set() {
        cleanup_position_stop_loss(position_key, position, client, median_priority_fee).await?;
    } else if position.take_profit_is_set() {
        cleanup_position_take_profit(position_key, position, client, median_priority_fee).await?;
    }
    Ok(())
}