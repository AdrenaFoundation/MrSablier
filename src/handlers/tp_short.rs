use {
    crate::{
        handlers::{create_ixs::create_close_position_short_ix, liquidate_long::calculate_size_risk_adjusted_position_fees},
        IndexedCustodiesThreadSafe, PriorityFeesThreadSafe, CLOSE_POSITION_SHORT_CU_LIMIT,
    },
    adrena_abi::{oracle::OraclePrice, Position, SPL_ASSOCIATED_TOKEN_PROGRAM_ID, SPL_TOKEN_PROGRAM_ID},
    anchor_client::Program,
    solana_client::rpc_config::RpcSendTransactionConfig,
    solana_sdk::{compute_budget::ComputeBudgetInstruction, pubkey::Pubkey, signature::Keypair},
    spl_associated_token_account::instruction::create_associated_token_account_idempotent,
    std::sync::Arc,
};

pub async fn tp_short(
    position_key: &Pubkey,
    position: &Position,
    oracle_price: &OraclePrice,
    indexed_custodies: &IndexedCustodiesThreadSafe,
    program: &Program<Arc<Keypair>>,
    priority_fees: &PriorityFeesThreadSafe,
    user_profile: Option<Pubkey>,
    referrer_profile: Option<Pubkey>,
) -> Result<(), backoff::Error<anyhow::Error>> {
    if position.take_profit_reached(oracle_price.price) {
        log::info!(
            "  <*> TP condition met for SHORT position {:#?} - Price: {}",
            position_key,
            oracle_price.price
        );
    } else {
        return Ok(());
    }

    let indexed_custodies_read = indexed_custodies.read().await;
    let custody = indexed_custodies_read.get(&position.custody).unwrap();
    let collateral_custody = indexed_custodies_read.get(&position.collateral_custody).unwrap();
    let oracle_pda = adrena_abi::pda::get_oracle_pda().0;

    let collateral_mint = collateral_custody.mint;

    let receiving_account = Pubkey::find_program_address(
        &[
            &position.owner.to_bytes(),
            &SPL_TOKEN_PROGRAM_ID.to_bytes(),
            &collateral_mint.to_bytes(),
        ],
        &SPL_ASSOCIATED_TOKEN_PROGRAM_ID,
    )
    .0;

    let transfer_authority_pda = adrena_abi::pda::get_transfer_authority_pda().0;

    let (close_position_short_ix, close_position_short_accounts) = create_close_position_short_ix(
        &program.payer(),
        position_key,
        position,
        receiving_account,
        transfer_authority_pda,
        user_profile,
        referrer_profile,
        &collateral_custody,
        &oracle_pda,
        None,
        position.take_profit_limit_price,
    );

    let priority_fee = calculate_size_risk_adjusted_position_fees(&position, &priority_fees).await?;

    let tx = program
        .request()
        .instruction(ComputeBudgetInstruction::set_compute_unit_price(priority_fee))
        .instruction(ComputeBudgetInstruction::set_compute_unit_limit(
            CLOSE_POSITION_SHORT_CU_LIMIT,
        ))
        .instruction(create_associated_token_account_idempotent(
            &program.payer(),
            &position.owner,
            &collateral_mint,
            &SPL_TOKEN_PROGRAM_ID,
        ))
        .args(close_position_short_ix)
        .accounts(close_position_short_accounts)
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
            log::error!("Transaction sending failed with error: {:?}", e);
            backoff::Error::transient(e.into())
        })?;

    log::info!(
        "  <> TP Short for position {:#?} - TX sent: {:#?}",
        position_key,
        tx_hash.to_string(),
    );
    // TODO wait for confirmation and retry if needed

    Ok(())
}
