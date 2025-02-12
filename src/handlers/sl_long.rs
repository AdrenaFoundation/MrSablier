use {
    crate::{handlers::create_close_position_long_ix, IndexedCustodiesThreadSafe, CLOSE_POSITION_LONG_CU_LIMIT},
    adrena_abi::{
        get_transfer_authority_pda, main_pool::USDC_CUSTODY_ID, oracle_price::OraclePrice, types::Cortex,
        Position, ADX_MINT, ALP_MINT, SPL_ASSOCIATED_TOKEN_PROGRAM_ID, SPL_TOKEN_PROGRAM_ID,
    },
    anchor_client::Program,
    solana_client::rpc_config::RpcSendTransactionConfig,
    solana_sdk::{compute_budget::ComputeBudgetInstruction, pubkey::Pubkey, signature::Keypair},
    spl_associated_token_account::instruction::create_associated_token_account_idempotent,
    std::sync::Arc,
};

pub async fn sl_long(
    position_key: &Pubkey,
    position: &Position,
    oracle_price: &OraclePrice,
    indexed_custodies: &IndexedCustodiesThreadSafe,
    program: &Program<Arc<Keypair>>,
    cortex: &Cortex,
    median_priority_fee: u64,
) -> Result<(), backoff::Error<anyhow::Error>> {
    if position.stop_loss_reached(oracle_price.price) {
        log::info!(
            "  <*> SL condition met for LONG position {:#?} - Price: {}",
            position_key,
            oracle_price.price
        );
    } else {
        return Ok(());
    }

    // check stop loss slippage is below 1% else return
    if !position.stop_loss_slippage_ok(oracle_price.price) {
        log::info!(
            "  <*> SL Long for position {:#?} - Price: {}",
            position_key,
            oracle_price.price
        );
    }

    let indexed_custodies_read = indexed_custodies.read().await;
    let custody = indexed_custodies_read.get(&position.custody).unwrap();
    let collateral_custody = indexed_custodies_read.get(&position.collateral_custody).unwrap();
    let staking_reward_token_custody = indexed_custodies_read.get(&USDC_CUSTODY_ID).unwrap();

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

    let transfer_authority_pda = get_transfer_authority_pda().0;
    let lm_staking = adrena_abi::pda::get_staking_pda(&ADX_MINT).0;
    let lp_staking = adrena_abi::pda::get_staking_pda(&ALP_MINT).0;

    let (close_position_long_params, close_position_long_accounts) = create_close_position_long_ix(
        &program.payer(),
        position_key,
        position,
        receiving_account,
        transfer_authority_pda,
        lm_staking,
        lp_staking,
        cortex,
        staking_reward_token_custody,
        custody,
        position.stop_loss_close_position_price,
    );

    let tx = program
        .request()
        .instruction(ComputeBudgetInstruction::set_compute_unit_price(median_priority_fee))
        .instruction(ComputeBudgetInstruction::set_compute_unit_limit(CLOSE_POSITION_LONG_CU_LIMIT))
        .instruction(create_associated_token_account_idempotent(
            &program.payer(),
            &position.owner,
            &collateral_mint,
            &SPL_TOKEN_PROGRAM_ID,
        ))
        .args(close_position_long_params)
        .accounts(close_position_long_accounts)
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
        "  <> SL Long for position {:#?} - TX sent: {:#?}",
        position_key,
        tx_hash.to_string(),
    );

    // TODO wait for confirmation and retry if needed

    Ok(())
}
