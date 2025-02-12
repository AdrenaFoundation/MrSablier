use {
    crate::{
        process_stream_message::{LimitOrderBookUpdate, PositionUpdate},
        IndexedCustodiesThreadSafe, IndexedLimitOrderBooksThreadSafe, IndexedPositionsThreadSafe,
    },
    adrena_abi::{AccountDeserialize, LimitOrderBook, Position},
    solana_sdk::{pubkey::Pubkey, signature::Keypair},
    std::{collections::HashSet, sync::Arc},
};

// Updates the indexed positions map based on the received account data.
// - Creates a new entry if the position is not indexed.
// - Deletes the entry if the account data is empty (position closed).
// - Updates the entry if the position is indexed and data is non-empty (position modified).
//
// Returns an enum with the update type and the position if it was created or modified
pub async fn update_indexed_positions(
    position_account_key: &Pubkey,
    position_account_data: &[u8],
    indexed_positions: &IndexedPositionsThreadSafe,
) -> Result<PositionUpdate, backoff::Error<anyhow::Error>> {
    let mut positions = indexed_positions.write().await;

    if position_account_data.is_empty() {
        positions.remove(position_account_key);
        return Ok(PositionUpdate::Closed);
    }

    let position = Position::try_deserialize(&mut &position_account_data[..]).map_err(|e| backoff::Error::transient(e.into()))?;

    let is_new_position = positions.insert(*position_account_key, position).is_none();

    if is_new_position {
        Ok(PositionUpdate::Created(position))
    } else {
        Ok(PositionUpdate::Modified(position))
    }
}

pub async fn update_indexed_limit_order_books(
    limit_order_book_account_key: &Pubkey,
    limit_order_book_account_data: &[u8],
    indexed_limit_order_books: &IndexedLimitOrderBooksThreadSafe,
) -> Result<LimitOrderBookUpdate, backoff::Error<anyhow::Error>> {
    let mut limit_order_books = indexed_limit_order_books.write().await;

    if limit_order_book_account_data.is_empty() {
        limit_order_books.remove(limit_order_book_account_key);
        return Ok(LimitOrderBookUpdate::Closed);
    }

    let limit_order_book = LimitOrderBook::try_deserialize(&mut &limit_order_book_account_data[..])
        .map_err(|e| backoff::Error::transient(e.into()))?;

    let is_new_limit_order_book = limit_order_books
        .insert(*limit_order_book_account_key, limit_order_book)
        .is_none();

    if is_new_limit_order_book {
        Ok(LimitOrderBookUpdate::Created(limit_order_book))
    } else {
        Ok(LimitOrderBookUpdate::Modified(limit_order_book))
    }
}

// Updates the indexed custodies map based on the indexed positions
pub async fn update_indexed_custodies(
    program: &anchor_client::Program<Arc<Keypair>>,
    indexed_positions: &IndexedPositionsThreadSafe,
    indexed_limit_order_books: &IndexedLimitOrderBooksThreadSafe,
    indexed_custodies: &IndexedCustodiesThreadSafe,
) -> Result<(), backoff::Error<anyhow::Error>> {
    let positions_custody_keys: HashSet<Pubkey> = indexed_positions.read().await.values().map(|p| p.custody).collect();
    let limit_order_book_custody_keys: HashSet<Pubkey> = indexed_limit_order_books
        .read()
        .await
        .values()
        .flat_map(|l| l.limit_orders.to_vec().into_iter().map(|l| l.custody))
        .collect();
    let limit_order_book_collateral_custody_keys: HashSet<Pubkey> = indexed_limit_order_books
        .read()
        .await
        .values()
        .flat_map(|l| l.limit_orders.to_vec().into_iter().map(|l| l.collateral_custody))
        .collect();

    let mut custody_keys = positions_custody_keys;
    custody_keys.extend(limit_order_book_custody_keys);
    custody_keys.extend(limit_order_book_collateral_custody_keys);

    log::info!(
        "  <> Existing positions/limit order books have {} unique custodies",
        custody_keys.len()
    );

    let mut custodies = indexed_custodies.write().await;

    for custody_key in custody_keys {
        let custody = program
            .account::<adrena_abi::types::Custody>(custody_key)
            .await
            .map_err(|e| backoff::Error::transient(e.into()))?;
        custodies.insert(custody_key, custody);
    }

    Ok(())
}
