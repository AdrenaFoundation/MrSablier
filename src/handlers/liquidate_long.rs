use {
    crate::{handlers::create_liquidate_long_ix, IndexedCustodiesThreadSafe, PriorityFeesThreadSafe, LIQUIDATE_LONG_CU_LIMIT},
    adrena_abi::{
        oracle_price::OraclePrice, LeverageCheckStatus, Pool, Position, SPL_ASSOCIATED_TOKEN_PROGRAM_ID, SPL_TOKEN_PROGRAM_ID,
    },
    anchor_client::Program,
    solana_client::rpc_config::RpcSendTransactionConfig,
    solana_sdk::{compute_budget::ComputeBudgetInstruction, pubkey::Pubkey, signature::Keypair},
    spl_associated_token_account::instruction::create_associated_token_account_idempotent,
    std::sync::Arc,
};

pub async fn liquidate_long(
    position_key: &Pubkey,
    position: &Position,
    oracle_price: &OraclePrice,
    indexed_custodies: &IndexedCustodiesThreadSafe,
    program: &Program<Arc<Keypair>>,
    pool: &Pool,
    priority_fees: &PriorityFeesThreadSafe,
    user_profile: Option<Pubkey>,
    referrer_profile: Option<Pubkey>,
) -> Result<(), backoff::Error<anyhow::Error>> {
    let current_time = chrono::Utc::now().timestamp();

    let indexed_custodies_read = indexed_custodies.read().await;
    let custody = indexed_custodies_read.get(&position.custody).unwrap();

    // determine the liquidation price
    let position_leverage_status = pool.check_leverage(
        &position,
        &oracle_price,
        &custody,
        &oracle_price,
        &custody,
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
                "  <*> Liquidation condition met for LONG position {:#?} - Oracle Price: {}, Position Leverage: {}",
                position_key,
                oracle_price.price,
                leverage
            );
        }
    };

    let indexed_custodies_read = indexed_custodies.read().await;
    let custody = indexed_custodies_read.get(&position.custody).unwrap();
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

    let (liquidate_long_ix, liquidate_long_accounts) = create_liquidate_long_ix(
        &program.payer(),
        position_key,
        position,
        receiving_account,
        transfer_authority_pda,
        custody,
        user_profile,
        referrer_profile,
    );

    let (_, priority_fee) = calculate_fee_risk_adjusted_position_fees(&position, &indexed_custodies, &priority_fees).await?;

    let tx = program
        .request()
        .instruction(ComputeBudgetInstruction::set_compute_unit_price(priority_fee))
        .instruction(ComputeBudgetInstruction::set_compute_unit_limit(LIQUIDATE_LONG_CU_LIMIT))
        .instruction(create_associated_token_account_idempotent(
            &program.payer(),
            &position.owner,
            &collateral_mint,
            &SPL_TOKEN_PROGRAM_ID,
        ))
        .args(liquidate_long_ix)
        .accounts(liquidate_long_accounts)
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
        "  <> Liquidated Long position {:#?} - TX sent: {:#?}",
        position_key,
        tx_hash.to_string(),
    );

    // TODO wait for confirmation and retry if needed

    Ok(())
}

// This function adjust which priority fees we will use based on the position's unrealized loss.
// Goal is to pay more fees for positions that will yield more fees to the protocol to make sure they lend
pub async fn calculate_fee_risk_adjusted_position_fees(
    position: &Position,
    indexed_custodies: &IndexedCustodiesThreadSafe,
    priority_fees: &PriorityFeesThreadSafe,
) -> Result<(u64, u64), anyhow::Error> {
    let indexed_custodies_read = indexed_custodies.read().await;
    let collateral_custody = indexed_custodies_read.get(&position.collateral_custody).unwrap().clone();

    let current_time = chrono::Utc::now().timestamp();
    let total_unrealized_interest_usd =
        collateral_custody.get_interest_amount_usd(position, current_time)? + position.unrealized_interest_usd;

    let unrealized_loss_usd = position.liquidation_fee_usd + total_unrealized_interest_usd;
    let priority_fees_read = priority_fees.read().await;
    let priority_fee = match unrealized_loss_usd {
        loss if loss > 2_500_000000 => {
            log::info!(
                "  <> -- Unrealized Loss: {}, Priority Fee: {} ULTRA",
                unrealized_loss_usd,
                priority_fees_read.ultra
            );
            priority_fees_read.ultra
        }
        loss if loss > 500_000000 => {
            log::info!(
                "  <> -- Unrealized Loss: {}, Priority Fee: {} HIGH",
                unrealized_loss_usd,
                priority_fees_read.high
            );
            priority_fees_read.high
        }
        _ => priority_fees_read.median,
    };

    Ok((unrealized_loss_usd, priority_fee))
}

// write a similar function that base the risk calculation on the position size (for SL/TP)
pub async fn calculate_size_risk_adjusted_position_fees(
    position: &Position,
    priority_fees: &PriorityFeesThreadSafe,
) -> Result<u64, anyhow::Error> {
    let priority_fees_read = priority_fees.read().await;
    let priority_fee = match position.borrow_size_usd {
        size if size > 1_000_000_000000 => {
            log::info!(
                "  <> -- Borrow Size: {}, Priority Fee: {} ULTRA",
                position.borrow_size_usd,
                priority_fees_read.ultra
            );
            priority_fees_read.ultra
        }
        loss if loss > 100_000_000000 => {
            log::info!(
                "  <> -- Borrow Size: {}, Priority Fee: {} HIGH",
                position.borrow_size_usd,
                priority_fees_read.high
            );
            priority_fees_read.high
        }
        _ => priority_fees_read.median,
    };

    Ok(priority_fee)
}
