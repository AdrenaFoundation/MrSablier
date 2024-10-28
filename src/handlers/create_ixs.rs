use {
    adrena_abi::{
        main_pool::USDC_CUSTODY_ID, types::Cortex, ALP_MINT, CORTEX_ID, SABLIER_THREAD_PROGRAM_ID,
        SPL_TOKEN_PROGRAM_ID,
    },
    solana_sdk::pubkey::Pubkey,
};

pub fn create_close_position_long_ix(
    payer: &Pubkey,
    position_key: &Pubkey,
    position: &adrena_abi::types::Position,
    receiving_account: Pubkey,
    transfer_authority_pda: Pubkey,
    lm_staking: Pubkey,
    lp_staking: Pubkey,
    cortex: &Cortex,
    user_profile: Option<Pubkey>,
    position_take_profit_pda: Pubkey,
    position_stop_loss_pda: Pubkey,
    staking_reward_token_custody: &adrena_abi::types::Custody,
    custody: &adrena_abi::types::Custody,
    limit_price: u64,
) -> (
    adrena_abi::instruction::ClosePositionLong,
    adrena_abi::accounts::ClosePositionLong,
) {
    let args = adrena_abi::instruction::ClosePositionLong {
        params: adrena_abi::types::ClosePositionLongParams {
            price: Some(limit_price),
        },
    };
    let accounts = adrena_abi::accounts::ClosePositionLong {
        caller: *payer,
        owner: position.owner,
        receiving_account,
        transfer_authority: transfer_authority_pda,
        lm_staking,
        lp_staking,
        cortex: CORTEX_ID,
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
            &lm_staking,
        )
        .0,
        lp_staking_reward_token_vault: adrena_abi::pda::get_staking_reward_token_vault_pda(
            &lp_staking,
        )
        .0,
        lp_token_mint: ALP_MINT,
        protocol_fee_recipient: cortex.protocol_fee_recipient,
        user_profile,
        take_profit_thread: position_take_profit_pda,
        stop_loss_thread: position_stop_loss_pda,
        token_program: SPL_TOKEN_PROGRAM_ID,
        adrena_program: adrena_abi::ID,
        sablier_program: SABLIER_THREAD_PROGRAM_ID,
    };
    (args, accounts)
}

pub fn create_close_position_short_ix(
    payer: &Pubkey,
    position_key: &Pubkey,
    position: &adrena_abi::types::Position,
    receiving_account: Pubkey,
    transfer_authority_pda: Pubkey,
    lm_staking: Pubkey,
    lp_staking: Pubkey,
    cortex: &Cortex,
    user_profile: Option<Pubkey>,
    position_take_profit_pda: Pubkey,
    position_stop_loss_pda: Pubkey,
    staking_reward_token_custody: &adrena_abi::types::Custody,
    custody: &adrena_abi::types::Custody,
    collateral_custody: &adrena_abi::types::Custody,
    limit_price: u64,
) -> (
    adrena_abi::instruction::ClosePositionShort,
    adrena_abi::accounts::ClosePositionShort,
) {
    let args = adrena_abi::instruction::ClosePositionShort {
        params: adrena_abi::types::ClosePositionShortParams {
            price: Some(limit_price),
        },
    };
    let accounts = adrena_abi::accounts::ClosePositionShort {
        caller: *payer,
        owner: position.owner,
        receiving_account,
        transfer_authority: transfer_authority_pda,
        lm_staking,
        lp_staking,
        cortex: CORTEX_ID,
        pool: position.pool,
        position: *position_key,
        staking_reward_token_custody: USDC_CUSTODY_ID,
        staking_reward_token_custody_oracle: staking_reward_token_custody.oracle,
        staking_reward_token_custody_token_account: staking_reward_token_custody.token_account,
        custody: position.custody,
        custody_trade_oracle: custody.trade_oracle,
        collateral_custody: position.collateral_custody,
        collateral_custody_oracle: collateral_custody.oracle,
        collateral_custody_token_account: collateral_custody.token_account,
        lm_staking_reward_token_vault: adrena_abi::pda::get_staking_reward_token_vault_pda(
            &lm_staking,
        )
        .0,
        lp_staking_reward_token_vault: adrena_abi::pda::get_staking_reward_token_vault_pda(
            &lp_staking,
        )
        .0,
        lp_token_mint: ALP_MINT,
        protocol_fee_recipient: cortex.protocol_fee_recipient,
        user_profile,
        take_profit_thread: position_take_profit_pda,
        stop_loss_thread: position_stop_loss_pda,
        token_program: SPL_TOKEN_PROGRAM_ID,
        adrena_program: adrena_abi::ID,
        sablier_program: SABLIER_THREAD_PROGRAM_ID,
    };
    (args, accounts)
}

pub fn create_liquidate_long_ix(
    payer: &Pubkey,
    position_key: &Pubkey,
    position: &adrena_abi::types::Position,
    receiving_account: Pubkey,
    transfer_authority_pda: Pubkey,
    lm_staking: Pubkey,
    lp_staking: Pubkey,
    cortex: &Cortex,
    user_profile: Option<Pubkey>,
    position_take_profit_pda: Pubkey,
    position_stop_loss_pda: Pubkey,
    staking_reward_token_custody: &adrena_abi::types::Custody,
    custody: &adrena_abi::types::Custody,
) -> (
    adrena_abi::instruction::LiquidateLong,
    adrena_abi::accounts::LiquidateLong,
) {
    let args = adrena_abi::instruction::LiquidateLong {
        params: adrena_abi::types::LiquidateLongParams {},
    };
    let accounts = adrena_abi::accounts::LiquidateLong {
        signer: *payer,
        receiving_account,
        transfer_authority: transfer_authority_pda,
        lm_staking,
        lp_staking,
        cortex: CORTEX_ID,
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
            &lm_staking,
        )
        .0,
        lp_staking_reward_token_vault: adrena_abi::pda::get_staking_reward_token_vault_pda(
            &lp_staking,
        )
        .0,
        lp_token_mint: ALP_MINT,
        protocol_fee_recipient: cortex.protocol_fee_recipient,
        user_profile,
        take_profit_thread: position_take_profit_pda,
        stop_loss_thread: position_stop_loss_pda,
        token_program: SPL_TOKEN_PROGRAM_ID,
        adrena_program: adrena_abi::ID,
        sablier_program: SABLIER_THREAD_PROGRAM_ID,
    };
    (args, accounts)
}

pub fn create_liquidate_short_ix(
    payer: &Pubkey,
    position_key: &Pubkey,
    position: &adrena_abi::types::Position,
    receiving_account: Pubkey,
    transfer_authority_pda: Pubkey,
    lm_staking: Pubkey,
    lp_staking: Pubkey,
    cortex: &Cortex,
    user_profile: Option<Pubkey>,
    position_take_profit_pda: Pubkey,
    position_stop_loss_pda: Pubkey,
    staking_reward_token_custody: &adrena_abi::types::Custody,
    custody: &adrena_abi::types::Custody,
    collateral_custody: &adrena_abi::types::Custody,
) -> (
    adrena_abi::instruction::LiquidateShort,
    adrena_abi::accounts::LiquidateShort,
) {
    let args = adrena_abi::instruction::LiquidateShort {
        params: adrena_abi::types::LiquidateShortParams {},
    };
    let accounts = adrena_abi::accounts::LiquidateShort {
        signer: *payer,
        receiving_account,
        transfer_authority: transfer_authority_pda,
        lm_staking,
        lp_staking,
        cortex: CORTEX_ID,
        pool: position.pool,
        position: *position_key,
        staking_reward_token_custody: USDC_CUSTODY_ID,
        staking_reward_token_custody_oracle: staking_reward_token_custody.oracle,
        staking_reward_token_custody_token_account: staking_reward_token_custody.token_account,
        custody: position.custody,
        custody_trade_oracle: custody.trade_oracle,
        collateral_custody: position.collateral_custody,
        collateral_custody_oracle: collateral_custody.oracle,
        collateral_custody_token_account: collateral_custody.token_account,
        lm_staking_reward_token_vault: adrena_abi::pda::get_staking_reward_token_vault_pda(
            &lm_staking,
        )
        .0,
        lp_staking_reward_token_vault: adrena_abi::pda::get_staking_reward_token_vault_pda(
            &lp_staking,
        )
        .0,
        lp_token_mint: ALP_MINT,
        protocol_fee_recipient: cortex.protocol_fee_recipient,
        user_profile,
        take_profit_thread: position_take_profit_pda,
        stop_loss_thread: position_stop_loss_pda,
        token_program: SPL_TOKEN_PROGRAM_ID,
        adrena_program: adrena_abi::ID,
        sablier_program: SABLIER_THREAD_PROGRAM_ID,
    };
    (args, accounts)
}

pub fn create_cleanup_position_stop_loss_ix(
    payer: &Pubkey,
    position_key: &Pubkey,
    owner: &Pubkey,
    cortex_key: &Pubkey,
    custody_key: &Pubkey,
    pool_key: &Pubkey,
    transfer_authority_pda: &Pubkey,
    position_take_profit_pda: &Pubkey,
    position_stop_loss_pda: &Pubkey,
) -> (
    adrena_abi::instruction::CleanupPositionStopLoss,
    adrena_abi::accounts::CleanupPositionStopLoss,
) {
    let args = adrena_abi::instruction::CleanupPositionStopLoss {};
    let accounts = adrena_abi::accounts::CleanupPositionStopLoss {
        caller: *payer,
        owner: *owner,
        transfer_authority: *transfer_authority_pda,
        cortex: *cortex_key,
        pool: *pool_key,
        position: *position_key,
        custody: *custody_key,
        stop_loss_thread: *position_stop_loss_pda,
        take_profit_thread: *position_take_profit_pda,
        sablier_program: SABLIER_THREAD_PROGRAM_ID,
    };
    (args, accounts)
}

pub fn create_cleanup_position_take_profit_ix(
    payer: &Pubkey,
    position_key: &Pubkey,
    owner: &Pubkey,
    cortex_key: &Pubkey,
    custody_key: &Pubkey,
    pool_key: &Pubkey,
    transfer_authority_pda: &Pubkey,
    position_take_profit_pda: &Pubkey,
    position_stop_loss_pda: &Pubkey,
) -> (
    adrena_abi::instruction::CleanupPositionTakeProfit,
    adrena_abi::accounts::CleanupPositionTakeProfit,
) {
    let args = adrena_abi::instruction::CleanupPositionTakeProfit {};
    let accounts = adrena_abi::accounts::CleanupPositionTakeProfit {
        caller: *payer,
        owner: *owner,
        transfer_authority: *transfer_authority_pda,
        cortex: *cortex_key,
        pool: *pool_key,
        position: *position_key,
        custody: *custody_key,
        stop_loss_thread: *position_stop_loss_pda,
        take_profit_thread: *position_take_profit_pda,
        sablier_program: SABLIER_THREAD_PROGRAM_ID,
    };
    (args, accounts)
}
