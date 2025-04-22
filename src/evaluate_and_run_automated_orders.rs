use {
    crate::{
        handlers::{self},
        IndexedCustodiesThreadSafe, IndexedLimitOrderBooksThreadSafe, IndexedPositionsThreadSafe, IndexedUserProfilesThreadSafe,
        PriorityFeesThreadSafe,
    },
    adrena_abi::{oracle::OraclePrice, Pool, Side},
    anchor_client::{Client, Cluster},
    solana_sdk::{pubkey::Pubkey, signature::Keypair},
    std::sync::Arc,
};

// Check liquidation/sl/tp/limit order conditions for a given asset (the last trading price inform which one)
pub async fn evaluate_and_run_automated_orders(
    indexed_positions: &IndexedPositionsThreadSafe,
    indexed_custodies: &IndexedCustodiesThreadSafe,
    indexed_limit_order_books: &IndexedLimitOrderBooksThreadSafe,
    indexed_user_profiles: &IndexedUserProfilesThreadSafe,
    payer: &Arc<Keypair>,
    endpoint: &str,
    pool: &Pool,
    priority_fees: &PriorityFeesThreadSafe,
    oracle_price: &OraclePrice,
) -> Result<(), backoff::Error<anyhow::Error>> {
    // Find the custody key associated with the trade oracle key
    let associated_custody_key = indexed_custodies
        .read()
        .await
        .iter()
        .find(|(_, c)| c.trade_oracle == oracle_price.name)
        .map(|(k, _)| *k)
        .ok_or(anyhow::anyhow!("No custody found for trade oracle key"))?;

    log::debug!(
        "    <> Pricefeed {:#?} update (price: {})",
        associated_custody_key,
        oracle_price.price
    );

    // check SL/TP/LIQ/LimitOrder conditions for all indexed positions/limit order books associated with the associated_custody_key
    // and that are not pending cleanup and close (just in case the position was partially handled by sablier)

    // make a clone of the indexed positions map to iterate over (while we modify the original map)
    let positions_shallow_clone = indexed_positions.read().await.clone();
    let mut tasks = vec![];

    for (position_key, position) in positions_shallow_clone
        .iter()
        .filter(|(_, p)| p.custody == associated_custody_key)
    {
        let position_key = *position_key;
        let position = *position;
        let indexed_custodies = Arc::clone(indexed_custodies);
        let pool = *pool;
        let priority_fees = Arc::clone(priority_fees);
        let indexed_user_profiles = Arc::clone(indexed_user_profiles);

        let client = Client::new(Cluster::Custom(endpoint.to_string(), endpoint.to_string()), Arc::clone(payer));
        let program = client
            .program(adrena_abi::ID)
            .map_err(|e| backoff::Error::transient(e.into()))?;

        // Only use user_profile when there is a referrer
        let (user_profile, referrer_profile) = {
            let indexed_user_profiles_read = indexed_user_profiles.read().await;
            let user_profile_pda =
                Pubkey::find_program_address(&[b"user_profile", &position.owner.to_bytes()], &adrena_abi::ID).0;

            // Use user_profile only when there is a referrer
            let user_profile = indexed_user_profiles_read.get(&user_profile_pda);

            if let Some(u) = user_profile {
                if u.referrer_profile.ne(&Pubkey::default()) {
                    (Some(user_profile_pda), Some(u.referrer_profile))
                } else {
                    (None, None)
                }
            } else {
                (None, None)
            }
        };

        let oracle_price = oracle_price.clone();
        let task = tokio::spawn(async move {
            let result: Result<(), anyhow::Error> = async {
                match position.get_side() {
                    Side::Long => {
                        // Check SL
                        if position.stop_loss_is_set() {
                            if let Err(e) = handlers::sl_long::sl_long(
                                &position_key,
                                &position,
                                &oracle_price,
                                &indexed_custodies,
                                &program,
                                &priority_fees,
                                user_profile,
                                referrer_profile,
                            )
                            .await
                            {
                                log::error!("Error in sl_long: {}", e);
                            }
                        }

                        // Check TP
                        if position.take_profit_is_set() && position.take_profit_limit_price != 0 {
                            if let Err(e) = handlers::tp_long::tp_long(
                                &position_key,
                                &position,
                                &oracle_price,
                                &indexed_custodies,
                                &program,
                                &priority_fees,
                                user_profile,
                                referrer_profile,
                            )
                            .await
                            {
                                log::error!("Error in tp_long: {}", e);
                            }
                        }

                        // Check LIQ
                        if let Err(e) = handlers::liquidate_long::liquidate_long(
                            &position_key,
                            &position,
                            &oracle_price,
                            &indexed_custodies,
                            &program,
                            &pool,
                            &priority_fees,
                            user_profile,
                            referrer_profile,
                        )
                        .await
                        {
                            log::error!("Error in liquidate_long: {}", e);
                        }
                    }
                    Side::Short => {
                        // Check SL
                        if position.stop_loss_is_set() {
                            if let Err(e) = handlers::sl_short::sl_short(
                                &position_key,
                                &position,
                                &oracle_price,
                                &indexed_custodies,
                                &program,
                                &priority_fees,
                                user_profile,
                                referrer_profile,
                            )
                            .await
                            {
                                log::error!("Error in sl_short: {}", e);
                            }
                        }

                        // Check TP
                        if position.take_profit_is_set() && position.take_profit_limit_price != 0 {
                            if let Err(e) = handlers::tp_short::tp_short(
                                &position_key,
                                &position,
                                &oracle_price,
                                &indexed_custodies,
                                &program,
                                &priority_fees,
                                user_profile,
                                referrer_profile,
                            )
                            .await
                            {
                                log::error!("Error in tp_short: {}", e);
                            }
                        }

                        // Check LIQ
                        if let Err(e) = handlers::liquidate_short::liquidate_short(
                            &position_key,
                            &position,
                            &oracle_price,
                            &indexed_custodies,
                            &program,
                            &pool,
                            &priority_fees,
                            user_profile,
                            referrer_profile,
                        )
                        .await
                        {
                            log::error!("Error in liquidate_short: {}", e);
                        }
                    }
                    _ => {}
                }
                Ok::<(), anyhow::Error>(())
            }
            .await;

            if let Err(e) = result {
                log::error!("Error processing position {}: {:?}", position_key, e);
            }
        });

        tasks.push(task);
    }

    // make a clone of the indexed positions map to iterate over (while we modify the original map)
    let limit_order_books_shallow_clone = indexed_limit_order_books.read().await.clone();
    let mut tasks = vec![];

    // let limit_order_book_custody_keys: HashSet<Pubkey> = indexed_limit_order_books
    // .read()
    // .await
    // .values()
    // .flat_map(|l| l.limit_orders.to_vec().into_iter().map(|l| l.custody))
    // .collect();

    for (limit_order_book_key, limit_order_book) in limit_order_books_shallow_clone.iter() {
        for limit_order in limit_order_book.limit_orders {
            if limit_order.custody == Pubkey::default() || limit_order.custody != associated_custody_key {
                continue;
            }

            let limit_order_book_key = *limit_order_book_key;
            let limit_order_book = *limit_order_book;
            let indexed_custodies = Arc::clone(indexed_custodies);
            let priority_fees = Arc::clone(priority_fees);

            let client = Client::new(Cluster::Custom(endpoint.to_string(), endpoint.to_string()), Arc::clone(payer));
            let program = client
                .program(adrena_abi::ID)
                .map_err(|e| backoff::Error::transient(e.into()))?;

            let oracle_price = oracle_price.clone();
            let task = tokio::spawn(async move {
                let result: Result<(), anyhow::Error> = async {
                    match limit_order.get_side() {
                        Side::Long => {
                            if limit_order.is_executable(&oracle_price, &associated_custody_key) {
                                if let Err(e) = handlers::execute_limit_order_long::execute_limit_order_long(
                                    &limit_order_book_key,
                                    &limit_order_book,
                                    &limit_order,
                                    &indexed_custodies,
                                    &program,
                                    &priority_fees,
                                )
                                .await
                                {
                                    log::error!("Error in execute_limit_order_long: {}", e);
                                }
                            }
                        }
                        Side::Short => {
                            if limit_order.is_executable(&oracle_price, &associated_custody_key) {
                                if let Err(e) = handlers::execute_limit_order_short::execute_limit_order_short(
                                    &limit_order_book_key,
                                    &limit_order_book,
                                    &limit_order,
                                    &indexed_custodies,
                                    &program,
                                    &priority_fees,
                                )
                                .await
                                {
                                    log::error!("Error in execute_limit_order_short: {}", e);
                                }
                            }
                        }
                        _ => {}
                    }
                    Ok::<(), anyhow::Error>(())
                }
                .await;

                if let Err(e) = result {
                    log::error!("Error processing position {}: {:?}", limit_order_book_key, e);
                }
            });

            tasks.push(task);
        }
    }

    // Wait for all tasks to complete
    for task in tasks {
        if let Err(e) = task.await {
            log::error!("Task failed: {:?}", e);
        }
    }
    Ok(())
}
