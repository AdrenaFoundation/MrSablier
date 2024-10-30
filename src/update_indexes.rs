use {
    crate::{
        process_stream_message::PositionUpdate, IndexedCustodiesThreadSafe,
        IndexedPositionsThreadSafe,
    },
    adrena_abi::{AccountDeserialize, Position},
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

    let position = Position::try_deserialize(&mut &position_account_data[..])
        .map_err(|e| backoff::Error::transient(e.into()))?;

    if position.is_pending_cleanup_and_close() {
        positions.remove(position_account_key);
        return Ok(PositionUpdate::PendingCleanupAndClose(position));
    }

    let is_new_position = positions.insert(*position_account_key, position).is_none();

    if is_new_position {
        Ok(PositionUpdate::Created(position))
    } else {
        Ok(PositionUpdate::Modified(position))
    }
}

// Updates the indexed custodies map based on the indexed positions
pub async fn update_indexed_custodies(
    program: &anchor_client::Program<Arc<Keypair>>,
    indexed_positions: &IndexedPositionsThreadSafe,
    indexed_custodies: &IndexedCustodiesThreadSafe,
) -> Result<(), backoff::Error<anyhow::Error>> {
    let custody_keys: HashSet<_> = indexed_positions
        .read()
        .await
        .values()
        .map(|p| p.custody)
        .collect();

    log::info!(
        "  <> Existing positions have {} unique custodies",
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
