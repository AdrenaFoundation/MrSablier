use {
    crate::{
        handlers::{create_liquidate_short_ix, liquidate_long::calculate_fee_risk_adjusted_position_fees},
        ChaosLabsBatchPricesThreadSafe, IndexedCustodiesThreadSafe, PriorityFeesThreadSafe, LIQUIDATE_SHORT_CU_LIMIT,
    },
    adrena_abi::{
        limited_string::LimitedString, oracle::OraclePrice, LeverageCheckStatus, Pool, Position, SPL_ASSOCIATED_TOKEN_PROGRAM_ID,
        SPL_TOKEN_PROGRAM_ID,
    },
    anchor_client::Program,
    solana_client::rpc_config::RpcSendTransactionConfig,
    solana_sdk::{compute_budget::ComputeBudgetInstruction, pubkey::Pubkey, signature::Keypair},
    spl_associated_token_account::instruction::create_associated_token_account_idempotent,
    std::sync::Arc,
};

pub async fn liquidate_short(
    position_key: &Pubkey,
    position: &Position,
    oracle_price: &OraclePrice,
    indexed_custodies: &IndexedCustodiesThreadSafe,
    program: &Program<Arc<Keypair>>,
    pool: &Pool,
    priority_fees: &PriorityFeesThreadSafe,
    user_profile: Option<Pubkey>,
    referrer_profile: Option<Pubkey>,
    oracle_prices: &ChaosLabsBatchPricesThreadSafe,
) -> Result<(), backoff::Error<anyhow::Error>> {
    let current_time = chrono::Utc::now().timestamp();

    let indexed_custodies_read = indexed_custodies.read().await;
    let custody = indexed_custodies_read.get(&position.custody).unwrap();
    let collateral_custody = indexed_custodies_read.get(&position.collateral_custody).unwrap();
    let oracle_pda = adrena_abi::pda::get_oracle_pda().0;

    // here we use the USDC price of 1 for simplicity
    let mock_collateral_token_price = OraclePrice::new(1_000_000, -6, 0, current_time, &LimitedString::new("whatever"));

    let position_leverage_status = pool.check_leverage(
        &position,
        &oracle_price,
        &custody,
        &mock_collateral_token_price,
        &collateral_custody,
        current_time,
        false,
    )?;

    match position_leverage_status {
        LeverageCheckStatus::Ok(leverage) => {
            if leverage > 2_500_000 {
                // 250x
                log::info!("  <*> Position {} nearing liquidation: {}", position_key, leverage);
                return Ok(());
            }
            // Silently return if leverage is below
            return Ok(());
        }
        LeverageCheckStatus::MaxLeverageExceeded(leverage) => {
            log::info!(
                "  <*> Liquidation condition met for SHORT position {:#?} - Oracle Price: {}, Position Leverage: {}",
                position_key,
                oracle_price.price,
                leverage
            );
        }
    };

    let indexed_custodies_read = indexed_custodies.read().await;
    let collateral_custody = indexed_custodies_read.get(&position.collateral_custody).unwrap();

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

    let (liquidate_short_ix, liquidate_short_accounts) = create_liquidate_short_ix(
        &program.payer(),
        position_key,
        position,
        receiving_account,
        transfer_authority_pda,
        user_profile,
        referrer_profile,
        &collateral_custody,
        &oracle_pda,
        Some(oracle_prices.read().await.clone()),
    );

    let (_, priority_fee) = calculate_fee_risk_adjusted_position_fees(&position, &indexed_custodies, &priority_fees).await?;

    let tx = program
        .request()
        .instruction(ComputeBudgetInstruction::set_compute_unit_price(priority_fee))
        .instruction(ComputeBudgetInstruction::set_compute_unit_limit(LIQUIDATE_SHORT_CU_LIMIT))
        .instruction(create_associated_token_account_idempotent(
            &program.payer(),
            &position.owner,
            &collateral_mint,
            &SPL_TOKEN_PROGRAM_ID,
        ))
        .args(liquidate_short_ix)
        .accounts(liquidate_short_accounts)
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
        "  <> Liquidated Short position {:#?} - TX sent: {:#?}",
        position_key,
        tx_hash.to_string(),
    );

    // TODO wait for confirmation and retry if needed

    Ok(())
}
