use {
    crate::{
        handlers::create_close_position_long_ix, utils, IndexedCustodiesThreadSafe,
        IndexedPositionsThreadSafe, CLOSE_POSITION_LONG_CU_LIMIT,
    },
    adrena_abi::{
        main_pool::USDC_CUSTODY_ID, types::Cortex, ADX_MINT, ALP_MINT,
        SPL_ASSOCIATED_TOKEN_PROGRAM_ID, SPL_TOKEN_PROGRAM_ID,
    },
    solana_client::rpc_config::RpcSendTransactionConfig,
    solana_sdk::{compute_budget::ComputeBudgetInstruction, pubkey::Pubkey, signature::Keypair},
    std::sync::Arc,
};

pub async fn tp_long(
    position_key: &Pubkey,
    position: &adrena_abi::types::Position,
    oracle_price: &utils::oracle_price::OraclePrice,
    indexed_custodies: &IndexedCustodiesThreadSafe,
    indexed_positions: &IndexedPositionsThreadSafe,
    program: &anchor_client::Program<Arc<Keypair>>,
    cortex: &Cortex,
    median_priority_fee: u64,
) -> Result<(), backoff::Error<anyhow::Error>> {
    // check if the price has crossed the TP
    if oracle_price.price >= position.take_profit_limit_price {
        log::info!("TP condition met for LONG position {:#?}", position_key);
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

    let (close_position_long_ix, close_position_long_accounts) = create_close_position_long_ix(
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
    );

    let tx = program
        .request()
        .instruction(ComputeBudgetInstruction::set_compute_unit_price(
            median_priority_fee,
        ))
        .instruction(ComputeBudgetInstruction::set_compute_unit_limit(
            CLOSE_POSITION_LONG_CU_LIMIT,
        ))
        .args(close_position_long_ix)
        .accounts(close_position_long_accounts)
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
        " TP Long for position {:#?} - TX sent: {:#?}",
        position_key,
        tx_hash.to_string(),
    );

    // TODO wait for confirmation and retry if needed

    // drop the position from the array
    indexed_positions
        .write()
        .await
        .retain(|p, _| p != position_key);
    log::info!("Position {:#?} de-indexed", position_key);

    Ok(())
}
