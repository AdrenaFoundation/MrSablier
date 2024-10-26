use {
    crate::{utils, IndexedCustodiesThreadSafe},
    adrena_abi::{
        main_pool::USDC_CUSTODY_ID, types::Cortex, ADX_MINT, ALP_MINT, SABLIER_THREAD_PROGRAM_ID,
        SPL_ASSOCIATED_TOKEN_PROGRAM_ID, SPL_TOKEN_PROGRAM_ID, USDC_MINT,
    },
    solana_sdk::{pubkey::Pubkey, signature::Keypair},
    std::rc::Rc,
};

pub async fn sl_long(
    position_key: &Pubkey,
    position: &adrena_abi::types::Position,
    oracle_price: &utils::oracle_price::OraclePrice,
    indexed_custodies: &IndexedCustodiesThreadSafe,
    program: &anchor_client::Program<Rc<Keypair>>,
    cortex: &Cortex,
) -> Result<(), backoff::Error<anyhow::Error>> {
    // check if the price has crossed the SL
    if oracle_price.price <= position.stop_loss_limit_price {
        log::info!("SL condition met for LONG position {:#?}", position_key);
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

    program
        .request()
        .args(adrena_abi::instruction::ClosePositionLong {
            params: adrena_abi::types::ClosePositionLongParams {
                price: Some(position.stop_loss_close_position_price),
            },
        })
        .accounts(adrena_abi::accounts::ClosePositionLong {
            caller: program.payer(),
            owner: position.owner,
            receiving_account,
            transfer_authority: transfer_authority_pda,
            lm_staking: adrena_abi::pda::get_staking_pda(&ADX_MINT).0,
            lp_staking: adrena_abi::pda::get_staking_pda(&USDC_MINT).0,
            cortex: adrena_abi::pda::get_cortex_pda().0,
            pool: position.pool,
            position: *position_key,
            staking_reward_token_custody: USDC_CUSTODY_ID,
            staking_reward_token_custody_oracle: staking_reward_token_custody.oracle,
            staking_reward_token_custody_token_account: staking_reward_token_custody.token_account,
            custody: position.custody,
            custody_oracle: custody.oracle,
            custody_trade_oracle: custody.trade_oracle,
            custody_token_account: custody.token_account,
            lm_staking_reward_token_vault: adrena_abi::pda::get_staking_reward_token_vault_pda(
                &ADX_MINT,
            )
            .0,
            lp_staking_reward_token_vault: adrena_abi::pda::get_staking_reward_token_vault_pda(
                &ALP_MINT,
            )
            .0,
            lp_token_mint: ALP_MINT,
            protocol_fee_recipient: cortex.protocol_fee_recipient,
            user_profile: Some(user_profile_pda),
            take_profit_thread: position_take_profit_pda,
            stop_loss_thread: position_stop_loss_pda,
            token_program: SPL_TOKEN_PROGRAM_ID,
            adrena_program: adrena_abi::ID,
            sablier_program: SABLIER_THREAD_PROGRAM_ID,
        })
        .send()
        .await
        .map_err(|e| backoff::Error::transient(e.into()))?;

    Ok(())
}
