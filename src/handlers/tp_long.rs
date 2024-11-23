use {
    crate::{handlers::sl_long::create_close_position_long_tx, IndexedCustodiesThreadSafe},
    adrena_abi::{
        main_pool::USDC_CUSTODY_ID, oracle_price::OraclePrice, types::Cortex, Position, ADX_MINT,
        ALP_MINT, SPL_ASSOCIATED_TOKEN_PROGRAM_ID, SPL_TOKEN_PROGRAM_ID,
    },
    anchor_client::Program,
    solana_client::rpc_config::RpcSendTransactionConfig,
    solana_sdk::{pubkey::Pubkey, signature::Keypair},
    std::sync::Arc,
};

pub async fn tp_long(
    position_key: &Pubkey,
    position: &Position,
    oracle_price: &OraclePrice,
    indexed_custodies: &IndexedCustodiesThreadSafe,
    program: &Program<Arc<Keypair>>,
    cortex: &Cortex,
    median_priority_fee: u64,
) -> Result<(), backoff::Error<anyhow::Error>> {
    if position.take_profit_reached(oracle_price.price) {
        log::info!(
            "   <*> TP condition met for LONG position {:#?} - Price: {}",
            position_key,
            oracle_price.price
        );
    } else {
        return Ok(());
    }

    let indexed_custodies_read = indexed_custodies.read().await;
    let custody = indexed_custodies_read.get(&position.custody).unwrap();
    let collateral_custody = indexed_custodies_read
        .get(&position.collateral_custody)
        .unwrap();
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

    let user_profile_pda = adrena_abi::pda::get_user_profile_pda(&position.owner).0;
    // Fetch the user profile account
    let user_profile_account = program.rpc().get_account(&user_profile_pda).await.ok(); // Convert Result to Option, None if error

    // Check if the user profile exists (owned by the Adrena program)
    let user_profile = match user_profile_account {
        Some(account) if account.owner == adrena_abi::ID => Some(user_profile_pda),
        _ => None,
    };

    let transfer_authority_pda = adrena_abi::pda::get_transfer_authority_pda().0;
    let lm_staking = adrena_abi::pda::get_staking_pda(&ADX_MINT).0;
    let lp_staking = adrena_abi::pda::get_staking_pda(&ALP_MINT).0;

    let tx_simulation = create_close_position_long_tx(
        program,
        position_key,
        position,
        receiving_account,
        transfer_authority_pda,
        lm_staking,
        lp_staking,
        cortex,
        user_profile,
        staking_reward_token_custody,
        custody,
        median_priority_fee,
        500_000,
    )
    .await?;

    let rpc_client = program.rpc();
    let mut simulation_attempts = 0;
    let simulation = loop {
        match rpc_client.simulate_transaction(&tx_simulation).await {
            Ok(simulation) => break simulation,
            Err(e) => {
                if e.to_string().contains("BlockhashNotFound") {
                    simulation_attempts += 1;
                    log::warn!(
                        "   <> Simulation attempt {} failed with error: {:?} - Retrying...",
                        simulation_attempts,
                        e
                    );
                    if simulation_attempts >= 25 {
                        return Err(backoff::Error::transient(e.into()));
                    }
                } else if e.to_string().contains("AccountOwnedByWrongProgram") {
                    // This means the position has already been closed by another instance

                    log::info!(
                        "   <> Position {:#?} has already been closed by another instance - Skipping",
                        position_key
                    );
                    return Ok(());
                } else {
                    log::error!("   <> Simulation failed with error: {:?}", e);
                    return Ok(());
                }
            }
        }
    };

    let simulated_cu = simulation.value.units_consumed.unwrap_or(0);

    if simulated_cu == 0 {
        log::warn!(
            "   <> CU consumed: {} - Simulation failed (possibly not executable yet)",
            simulated_cu
        );
        return Ok(());
    }

    let tx = create_close_position_long_tx(
        program,
        position_key,
        position,
        receiving_account,
        transfer_authority_pda,
        lm_staking,
        lp_staking,
        cortex,
        user_profile,
        staking_reward_token_custody,
        custody,
        median_priority_fee,
        (simulated_cu as f64 * 1.05) as u32,
    )
    .await?;

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
            log::error!("   <> Transaction sending failed with error: {:?}", e);
            backoff::Error::transient(e.into())
        })?;

    log::info!(
        "   <> TP Long for position {:#?} - TX sent: {:#?}",
        position_key,
        tx_hash.to_string(),
    );

    Ok(())
}
