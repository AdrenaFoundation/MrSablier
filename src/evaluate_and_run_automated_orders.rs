use {
    crate::{
        handlers, program_wrapper::ProgramWrapper, IndexedCustodiesThreadSafe, IndexedLimitOrderBooksThreadSafe,
        IndexedPositionsThreadSafe, PriorityFeesThreadSafe,
    },
    adrena_abi::{
        oracle_price::OraclePrice,
        pyth::PriceUpdateV2,
        types::{Cortex, Side},
        Pool,
    },
    solana_sdk::pubkey::Pubkey,
    std::sync::Arc,
    tokio::sync::RwLock,
};

// Check liquidation/sl/tp/limit order conditions for related positions
// Based on the provided price_update_v2 account, check if any of the related positions have crossed their liquidation/sl/tp conditions
// First go over the price update v2 account to get the price, then go over the indexed positions to check if any of them have crossed their conditions
pub async fn evaluate_and_run_automated_orders(
    trade_oracle_key: &Pubkey,
    trade_oracle_data: &[u8],
    indexed_positions: &IndexedPositionsThreadSafe,
    indexed_custodies: &IndexedCustodiesThreadSafe,
    indexed_limit_order_books: &IndexedLimitOrderBooksThreadSafe,
    program: &ProgramWrapper,
    cortex: &Cortex,
    pool: &Pool,
    priority_fees: &PriorityFeesThreadSafe,
    sol_price: &Arc<RwLock<f64>>,
) -> Result<(), backoff::Error<anyhow::Error>> {
    // Deserialize price update v2 account
    let trade_oracle: PriceUpdateV2 =
        borsh::BorshDeserialize::deserialize(&mut &trade_oracle_data[8..]).map_err(|e| backoff::Error::transient(e.into()))?;

    // TODO: Optimize this by not creating the OraclePrice struct from the price update v2 account but just using the price and conf directly
    // Create an OraclePrice struct from the price update v2 account
    let oracle_price: OraclePrice =
        OraclePrice::new_from_pyth_price_update_v2(&trade_oracle).map_err(backoff::Error::transient)?;

    // Find the custody key associated with the trade oracle key
    let associated_custody_key = indexed_custodies
        .read()
        .await
        .iter()
        .find(|(_, c)| c.trade_oracle == *trade_oracle_key)
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

    // Create a clone of the program wrapper for use in spawned tasks
    let program_for_tasks = program.clone();
    let mut tasks = vec![];

    for (position_key, position) in positions_shallow_clone
        .iter()
        .filter(|(_, p)| p.custody == associated_custody_key)
    {
        let position_key = *position_key;
        let position = *position;
        let indexed_custodies = Arc::clone(indexed_custodies);
        let cortex = *cortex;
        let pool = *pool;
        let priority_fees = Arc::clone(priority_fees);
        let sol_price = Arc::clone(sol_price);
        let program = program_for_tasks.clone();

        let task = tokio::spawn(async move {
            let result: Result<(), anyhow::Error> = async {
                match position.get_side() {
                    Side::Long => {
                        // Check LIQ
                        if let Err(e) = handlers::liquidate_long::liquidate_long(
                            &position_key,
                            &position,
                            &oracle_price,
                            &indexed_custodies,
                            program.get_program(),
                            &cortex,
                            &pool,
                            &priority_fees,
                            &sol_price,
                        )
                        .await
                        {
                            log::error!("Error in liquidate_long: {}", e);
                        }
                    }
                    Side::Short => {
                        // Check LIQ - uncomment when ready
                        /* if let Err(e) = handlers::liquidate_short::liquidate_short(
                            &position_key,
                            &position,
                            &oracle_price,
                            &indexed_custodies,
                            program.get_program(),
                            &cortex,
                            &pool,
                            &priority_fees,
                        )
                        .await
                        {
                            log::error!("Error in liquidate_short: {}", e);
                        } */
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

    // Wait for all tasks to complete
    for task in tasks {
        if let Err(e) = task.await {
            log::error!("Task failed: {:?}", e);
        }
    }

    Ok(())
}
