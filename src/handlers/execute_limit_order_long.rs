use {
    crate::{
        handlers::create_execute_limit_order_long_ix, IndexedCustodiesThreadSafe, PriorityFeesThreadSafe,
        EXECUTE_LIMIT_ORDER_LONG_CU_LIMIT,
    },
    adrena_abi::{LimitOrder, LimitOrderBook},
    anchor_client::Program,
    solana_client::rpc_config::RpcSendTransactionConfig,
    solana_sdk::{compute_budget::ComputeBudgetInstruction, pubkey::Pubkey, signature::Keypair},
    std::sync::Arc,
};

pub async fn execute_limit_order_long(
    limit_order_book_key: &Pubkey,
    limit_order_book: &LimitOrderBook,
    limit_order: &LimitOrder,
    indexed_custodies: &IndexedCustodiesThreadSafe,
    program: &Program<Arc<Keypair>>,
    priority_fees: &PriorityFeesThreadSafe,
) -> Result<(), backoff::Error<anyhow::Error>> {
    let indexed_custodies_read = indexed_custodies.read().await;
    let custody = indexed_custodies_read.get(&limit_order.custody).unwrap();
    let collateral_custody = indexed_custodies_read.get(&limit_order.collateral_custody).unwrap();

    let pool_pda = adrena_abi::pda::get_pool_pda(&String::from("main-pool")).0;

    let limit_order_book_pda = adrena_abi::pda::get_limit_order_book_pda(&pool_pda, &limit_order_book.owner).0;
    let collateral_escrow_pda =
        adrena_abi::pda::get_collateral_escrow_pda(&pool_pda, &limit_order_book.owner, &collateral_custody.mint).0;

    let position_pda = adrena_abi::pda::get_position_pda(
        &limit_order_book.owner,
        &pool_pda,
        &limit_order.custody,
        limit_order.get_side(),
    )
    .0;

    let transfer_authority_pda = adrena_abi::pda::get_transfer_authority_pda().0;

    let (execute_limit_order_long_ix, execute_limit_order_long_ix_accounts) = create_execute_limit_order_long_ix(
        &program.payer(),
        &limit_order_book.owner,
        &position_pda,
        &pool_pda,
        &transfer_authority_pda,
        &collateral_escrow_pda,
        &limit_order_book_pda,
        &limit_order.custody,
        custody,
        limit_order.id,
    );

    let priority_fees_read = priority_fees.read().await;

    let tx = program
        .request()
        .instruction(ComputeBudgetInstruction::set_compute_unit_price(priority_fees_read.high))
        .instruction(ComputeBudgetInstruction::set_compute_unit_limit(
            EXECUTE_LIMIT_ORDER_LONG_CU_LIMIT,
        ))
        .args(execute_limit_order_long_ix)
        .accounts(execute_limit_order_long_ix_accounts)
        .signed_transaction()
        .await
        .map_err(|e| {
            log::error!("  <> Transaction generation failed with error: {:?}", e);
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
            log::error!("  <> Transaction sending failed with error: {:?}", e);
            backoff::Error::transient(e.into())
        })?;

    log::info!(
        "  <> Executed Limit Order Long {:#?} id {:?} - TX sent: {:#?}",
        limit_order_book_key.to_string(),
        limit_order.id,
        tx_hash.to_string(),
    );

    // TODO wait for confirmation and retry if needed

    Ok(())
}
