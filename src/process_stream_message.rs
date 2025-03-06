use {
    crate::{
        evaluate_and_run_automated_orders::evaluate_and_run_automated_orders,
        update_indexes::{update_indexed_limit_order_books, update_indexed_positions},
        IndexedCustodiesThreadSafe, IndexedLimitOrderBooksThreadSafe, IndexedPositionsThreadSafe, PriorityFeesThreadSafe,
        SolPriceThreadSafe,
    },
    adrena_abi::{
        types::{Cortex, Position},
        LimitOrderBook, Pool,
    },
    anchor_client::{Client, Cluster},
    solana_sdk::{pubkey::Pubkey, signature::Keypair},
    std::sync::Arc,
    yellowstone_grpc_proto::geyser::{subscribe_update::UpdateOneof, SubscribeUpdate},
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

pub async fn process_stream_message(
    message: Result<SubscribeUpdate, backoff::Error<anyhow::Error>>,
    indexed_positions: &IndexedPositionsThreadSafe,
    indexed_custodies: &IndexedCustodiesThreadSafe,
    indexed_limit_order_books: &IndexedLimitOrderBooksThreadSafe,
    payer: &Arc<Keypair>,
    endpoint: &str,
    cortex: &Cortex,
    pool: &Pool,
    priority_fees: &PriorityFeesThreadSafe,
    sol_price: &SolPriceThreadSafe,
) -> Result<(), backoff::Error<anyhow::Error>> {
    let program = Client::new(Cluster::Custom(endpoint.to_string(), endpoint.to_string()), Arc::clone(payer))
        .program(adrena_abi::ID)
        .map_err(|e| backoff::Error::transient(e.into()))?;

    match message {
        Ok(msg) => match msg.update_oneof {
            Some(UpdateOneof::Account(sua)) => {
                let account = sua.account.expect("Account should be defined");
                let account_key = Pubkey::try_from(account.pubkey).expect("valid pubkey");
                let account_data = account.data.to_vec();

                if msg.filters.contains(&"price_feeds".to_owned()) {
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
                        sol_price,
                    )
                    .await?;
                }
                if msg.filters.contains(&"positions_create_update".to_owned()) {
                    let update = update_indexed_positions(&account_key, &account_data, indexed_positions).await?;

                    match update {
                        PositionUpdate::Created(new_position) => {
                            log::info!("(pcu) New position created: {:#?}", account_key);
                            if !indexed_custodies.read().await.contains_key(&new_position.custody) {
                                let custody = program
                                    .account::<adrena_abi::types::Custody>(new_position.custody)
                                    .await
                                    .map_err(|e| backoff::Error::transient(e.into()))?;

                                indexed_custodies.write().await.insert(new_position.custody, custody);
                                log::info!("(pcu) Fetched new custody {:#?} from RPC", new_position.custody);
                            }
                        }
                        PositionUpdate::Modified(_position) => {
                            log::info!("(pcu) Position modified: {:#?}", account_key);
                        }
                        PositionUpdate::Closed => {
                            log::info!("(pcu) Position closed: {:#?}", account_key);
                        }
                    }
                } else if msg.filters.contains(&"positions_close".to_owned()) {
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
                        }
                    }
                }
            }
            Some(UpdateOneof::Ping(_)) => {
                log::debug!("  <> Received ping message");
            }
            _ => {}
        },
        Err(error) => {
            log::error!("error: {error:?}");
            return Err(error);
        }
    }

    Ok(())
}
