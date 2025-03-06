use {
    crate::{handlers::create_liquidate_long_ix, IndexedCustodiesThreadSafe, PriorityFeesThreadSafe, LIQUIDATE_LONG_CU_LIMIT},
    adrena_abi::{
        main_pool::USDC_CUSTODY_ID, oracle_price::OraclePrice, types::Cortex, LeverageCheckStatus, Pool, Position, ADX_MINT,
        ALP_MINT, SPL_ASSOCIATED_TOKEN_PROGRAM_ID, SPL_TOKEN_PROGRAM_ID,
    },
    anchor_client::Program,
    bincode,
    solana_client::{
        client_error::{ClientError, ClientErrorKind},
        nonblocking::rpc_client::RpcClient,
        rpc_config::RpcSendTransactionConfig,
    },
    solana_sdk::{
        compute_budget::ComputeBudgetInstruction,
        pubkey::Pubkey,
        signature::{Keypair, Signature},
        transaction::{Transaction, TransactionError},
    },
    spl_associated_token_account::instruction::create_associated_token_account_idempotent,
    std::{sync::Arc, time::Duration},
    tokio::sync::RwLock,
};

pub async fn liquidate_long(
    position_key: &Pubkey,
    position: &Position,
    oracle_price: &OraclePrice,
    indexed_custodies: &IndexedCustodiesThreadSafe,
    program: &Program<Arc<Keypair>>,
    cortex: &Cortex,
    pool: &Pool,
    priority_fees: &PriorityFeesThreadSafe,
    sol_price: &Arc<RwLock<f64>>,
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

    let transfer_authority_pda = adrena_abi::pda::get_transfer_authority_pda().0;
    let lm_staking = adrena_abi::pda::get_staking_pda(&ADX_MINT).0;
    let lp_staking = adrena_abi::pda::get_staking_pda(&ALP_MINT).0;

    let (liquidate_long_ix, liquidate_long_accounts) = create_liquidate_long_ix(
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
    );

    let (unrealized_loss_usd, initial_priority_fee) =
        calculate_fee_risk_adjusted_position_fees(&position, &indexed_custodies, &priority_fees).await?;

    let tx = program
        .request()
        .instruction(ComputeBudgetInstruction::set_compute_unit_price(initial_priority_fee))
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

    let tx_hash = send_transaction_with_fee_escalation(
        &rpc_client,
        tx,
        initial_priority_fee,
        5, // number of attempts to send the transaction, makes gradual steps
        unrealized_loss_usd,
        sol_price,
    )
    .await?;

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

pub async fn send_transaction_with_fee_escalation(
    rpc_client: &RpcClient,
    base_tx: Transaction,
    initial_fee: u64,
    max_attempts: u32,
    unrealized_loss_usd: u64,
    sol_price: &Arc<RwLock<f64>>,
) -> Result<Signature, backoff::Error<anyhow::Error>> {
    let mut current_fee = initial_fee;
    let mut attempt = 0;

    // Get the current SOL price
    let sol_price_value = *sol_price.read().await;

    // Calculate max fee as 1% of unrealized loss
    let max_fee = {
        let loss_in_sol = unrealized_loss_usd as f64 / (sol_price_value * 1_000_000.0); // Convert USD to SOL using actual price
        let one_percent = loss_in_sol / 100.0;
        let micro_lamports = (one_percent * 1_000_000_000.0 * 1_000.0) as u64;
        micro_lamports / LIQUIDATE_LONG_CU_LIMIT as u64
    };

    // Calculate fee increase step to reach max_fee in 5 attempts to avoid spamming the network
    let fee_step: u64 = if max_fee > initial_fee {
        (max_fee - initial_fee) / (max_attempts as u64)
    } else {
        0 // If max_fee is less than initial_fee, don't increase
    };

    log::info!(
        "  <> Fee escalation strategy: initial: {}, step: {}, max: {}, SOL price: ${:.2}",
        initial_fee,
        fee_step,
        max_fee,
        sol_price_value
    );

    while attempt < max_attempts {
        current_fee = initial_fee + (fee_step * attempt as u64);
        current_fee = current_fee.min(max_fee);

        let mut tx = base_tx.clone();
        tx.message.recent_blockhash = rpc_client
            .get_latest_blockhash()
            .await
            .map_err(|e| backoff::Error::transient(e.into()))?;

        if let Some(ix) = tx.message.instructions.get_mut(0) {
            ix.data = bincode::serialize(&ComputeBudgetInstruction::set_compute_unit_price(current_fee))
                .map_err(|e| backoff::Error::permanent(e.into()))?;
        }

        match rpc_client
            .send_transaction_with_config(
                &tx,
                RpcSendTransactionConfig {
                    skip_preflight: true,
                    max_retries: Some(0),
                    ..Default::default()
                },
            )
            .await
        {
            Ok(signature) => {
                log::info!(
                    "  <> Transaction landed successfully on attempt {} with fee {} (step: {}, max: {})",
                    attempt + 1,
                    current_fee,
                    fee_step,
                    max_fee
                );
                return Ok(signature);
            }
            Err(e) => {
                log::warn!(
                    "  <> Transaction failed on attempt {} with fee {} (step: {}, max: {}): {:?} - retrying",
                    attempt + 1,
                    current_fee,
                    fee_step,
                    max_fee,
                    &e
                );

                let should_retry = match &e {
                    ClientError {
                        kind: ClientErrorKind::TransactionError(tx_error),
                        ..
                    } => {
                        match tx_error {
                            // Only retry on fee-related errors
                            TransactionError::WouldExceedMaxBlockCostLimit
                            | TransactionError::WouldExceedMaxAccountCostLimit
                            | TransactionError::WouldExceedAccountDataBlockLimit
                            | TransactionError::WouldExceedMaxVoteCostLimit => true,

                            // Don't retry on any other transaction error
                            _ => false,
                        }
                    }
                    // Don't retry on any other error type
                    _ => false,
                };

                if !should_retry {
                    return Err(backoff::Error::permanent(e.into()));
                }

                attempt += 1;
                if attempt < max_attempts {
                    // Exponential backoff for network errors
                    let delay = Duration::from_millis(500 * (attempt as u64));
                    tokio::time::sleep(delay).await;
                }
            }
        }
    }

    Err(backoff::Error::permanent(anyhow::anyhow!(
        "Failed to land transaction after {} attempts, final fee: {} (step: {}, max: {})",
        max_attempts,
        current_fee,
        fee_step,
        max_fee
    )))
}
