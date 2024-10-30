use {
    crate::{
        handlers::create_liquidate_short_ix, CachedLiquidationData, IndexedCustodiesThreadSafe,
        LiquidationPriceCacheThreadSafe, LIQUIDATE_SHORT_CU_LIMIT, MAX_LIQUIDATION_PRICE_CACHE_AGE,
    },
    adrena_abi::{
        liquidation_price::get_liquidation_price, main_pool::USDC_CUSTODY_ID,
        oracle_price::OraclePrice, types::Cortex, Custody, Position, ADX_MINT, ALP_MINT,
        SPL_ASSOCIATED_TOKEN_PROGRAM_ID, SPL_TOKEN_PROGRAM_ID,
    },
    anchor_client::Client,
    solana_client::rpc_config::RpcSendTransactionConfig,
    solana_sdk::{compute_budget::ComputeBudgetInstruction, pubkey::Pubkey, signature::Keypair},
    std::sync::Arc,
};

pub async fn liquidate_short(
    position_key: &Pubkey,
    position: &Position,
    oracle_price: &OraclePrice,
    indexed_custodies: &IndexedCustodiesThreadSafe,
    liquidation_price_cache: &LiquidationPriceCacheThreadSafe,
    client: &Client<Arc<Keypair>>,
    cortex: &Cortex,
    median_priority_fee: u64,
) -> Result<(), backoff::Error<anyhow::Error>> {
    let current_time = chrono::Utc::now().timestamp();

    let indexed_custodies_read = indexed_custodies.read().await;
    let custody = indexed_custodies_read.get(&position.custody).unwrap();
    let collateral_custody = indexed_custodies_read
        .get(&position.collateral_custody)
        .unwrap();

    // Determine liquidation price - Optimized with a cache
    let liquidation_price = {
        let cache = liquidation_price_cache.read().await;
        let cached_data = cache.get(position_key).cloned();
        drop(cache);

        match cached_data {
            Some(cached_data)
                if should_use_cached_price(&cached_data, current_time, oracle_price) =>
            {
                cached_data.liquidation_price
            }
            _ => {
                let new_price = calculate_and_update_cache(
                    position_key,
                    position,
                    custody,
                    collateral_custody,
                    oracle_price,
                    current_time,
                    &liquidation_price_cache,
                )
                .await?;
                new_price
            }
        }
    };

    // check if the price has crossed the liquidation price
    if oracle_price.price >= liquidation_price {
        log::info!(
            "Liquidation condition met for SHORT position {:#?} - Oracle Price: {}, Liquidation Price: {}",
            position_key,
            oracle_price.price,
            liquidation_price
        );
    } else {
        return Ok(());
    }
    let program = client
        .program(adrena_abi::ID)
        .map_err(|e| backoff::Error::transient(e.into()))?;

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

    let position_take_profit_pda = adrena_abi::pda::get_sablier_thread_pda(
        &transfer_authority_pda,
        position.take_profit_thread_id.to_le_bytes().to_vec(),
        Some(position.owner.to_bytes().to_vec()),
    )
    .0;

    let position_stop_loss_pda = adrena_abi::pda::get_sablier_thread_pda(
        &transfer_authority_pda,
        position.stop_loss_thread_id.to_le_bytes().to_vec(),
        Some(position.owner.to_bytes().to_vec()),
    )
    .0;

    let lm_staking = adrena_abi::pda::get_staking_pda(&ADX_MINT).0;
    let lp_staking = adrena_abi::pda::get_staking_pda(&ALP_MINT).0;

    let (liquidate_short_ix, liquidate_short_accounts) = create_liquidate_short_ix(
        &program.payer(),
        position_key,
        position,
        receiving_account,
        transfer_authority_pda,
        lm_staking,
        lp_staking,
        cortex,
        user_profile,
        position_take_profit_pda,
        position_stop_loss_pda,
        staking_reward_token_custody,
        custody,
        collateral_custody,
    );

    let tx = program
        .request()
        .instruction(ComputeBudgetInstruction::set_compute_unit_price(
            median_priority_fee,
        ))
        .instruction(ComputeBudgetInstruction::set_compute_unit_limit(
            LIQUIDATE_SHORT_CU_LIMIT,
        ))
        .args(liquidate_short_ix)
        .accounts(liquidate_short_accounts)
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
        "Liquidated Short position {:#?} - TX sent: {:#?}",
        position_key,
        tx_hash.to_string(),
    );

    // TODO wait for confirmation and retry if needed

    Ok(())
}

fn should_use_cached_price(
    cached_data: &CachedLiquidationData,
    current_time: i64,
    oracle_price: &OraclePrice,
) -> bool {
    current_time - cached_data.last_updated < MAX_LIQUIDATION_PRICE_CACHE_AGE
        || oracle_price.price > cached_data.price
}

async fn calculate_and_update_cache(
    position_key: &Pubkey,
    position: &Position,
    custody: &Custody,
    collateral_custody: &Custody,
    oracle_price: &OraclePrice,
    current_time: i64,
    cache: &LiquidationPriceCacheThreadSafe,
) -> Result<u64, anyhow::Error> {
    let new_price = get_liquidation_price(position, custody, collateral_custody, current_time)?;
    cache.write().await.insert(
        *position_key,
        CachedLiquidationData {
            price: oracle_price.price,
            liquidation_price: new_price,
            last_updated: current_time,
        },
    );
    Ok(new_price)
}
