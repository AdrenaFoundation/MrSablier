use {
    crate::{
        handlers::{
            create_cleanup_position_take_profit_ix,
            create_ixs::create_cleanup_position_stop_loss_ix,
        },
        utils::get_sablier_thread_pda,
        CLEANUP_POSITION_CU_LIMIT,
    },
    anchor_client::Client,
    solana_client::rpc_config::RpcSendTransactionConfig,
    solana_sdk::{compute_budget::ComputeBudgetInstruction, pubkey::Pubkey, signature::Keypair},
    std::sync::Arc,
};

/// This function cleans both in a single tx to prevent unnecessary modification updates in this script
pub async fn cleanup_position(
    position_key: &Pubkey,
    position: &adrena_abi::types::Position,
    client: &Client<Arc<Keypair>>,
    median_priority_fee: u64,
) -> Result<(), backoff::Error<anyhow::Error>> {
    if position.pending_cleanup_and_close != 1 {
        return Ok(());
    }

    log::info!("Cleanup SL/TP for position {:#?}", position_key,);

    let program = client
        .program(adrena_abi::ID)
        .map_err(|e| backoff::Error::transient(e.into()))?;

    let transfer_authority_pda = adrena_abi::pda::get_transfer_authority_pda().0;
    let position_take_profit_pda = get_sablier_thread_pda(
        &transfer_authority_pda,
        position.take_profit_thread_id,
        &position.owner,
    );
    let position_stop_loss_pda = get_sablier_thread_pda(
        &transfer_authority_pda,
        position.stop_loss_thread_id,
        &position.owner,
    );

    let (cleanup_position_stop_loss_ix, cleanup_position_stop_loss_accounts) =
        create_cleanup_position_stop_loss_ix(
            &program.payer(),
            position_key,
            &position.owner,
            &adrena_abi::CORTEX_ID,
            &position.custody,
            &position.pool,
            &transfer_authority_pda,
            &position_take_profit_pda,
            &position_stop_loss_pda,
        );

    let (cleanup_position_take_profit_ix, cleanup_position_take_profit_accounts) =
        create_cleanup_position_take_profit_ix(
            &program.payer(),
            position_key,
            &position.owner,
            &adrena_abi::CORTEX_ID,
            &position.custody,
            &position.pool,
            &transfer_authority_pda,
            &position_take_profit_pda,
            &position_stop_loss_pda,
        );

    let cleanup_position_stop_loss_ix = program
        .request()
        .args(cleanup_position_stop_loss_ix)
        .accounts(cleanup_position_stop_loss_accounts)
        .instructions()
        .unwrap();

    let cleanup_position_take_profit_ix = program
        .request()
        .args(cleanup_position_take_profit_ix)
        .accounts(cleanup_position_take_profit_accounts)
        .instructions()
        .unwrap();

    let tx = program
        .request()
        .instruction(ComputeBudgetInstruction::set_compute_unit_price(
            median_priority_fee,
        ))
        .instruction(ComputeBudgetInstruction::set_compute_unit_limit(
            CLEANUP_POSITION_CU_LIMIT * 2,
        ))
        .instruction(cleanup_position_stop_loss_ix.first().unwrap().clone())
        .instruction(cleanup_position_take_profit_ix.first().unwrap().clone())
        .signed_transaction()
        .await
        .map_err(|e| {
            log::error!("Transaction generation failed with error: {:?}", e);
            backoff::Error::transient(e.into())
        })?;

    let rpc_client = program.rpc();

    let tx_hash = rpc_client
        .send_transaction_with_config(
            &tx,
            RpcSendTransactionConfig {
                skip_preflight: true,
                max_retries: Some(0),
                ..Default::default()
            },
        )
        .await
        .map_err(|e| {
            log::error!("Transaction sending failed with error: {:?}", e);
            backoff::Error::transient(e.into())
        })?;

    log::info!(
        "Cleanup position SL/TP for position {:#?} - TX sent: {:#?}",
        position_key,
        tx_hash.to_string(),
    );
    // TODO wait for confirmation and retry if needed

    Ok(())
}
