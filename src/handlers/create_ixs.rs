use {
    adrena_abi::{main_pool::USDC_CUSTODY_ID, types::Cortex, ALP_MINT, CORTEX_ID, SPL_TOKEN_PROGRAM_ID, SYSTEM_PROGRAM_ID},
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
    staking_reward_token_custody: &adrena_abi::types::Custody,
    custody: &adrena_abi::types::Custody,
    limit_price: u64,
) -> (
    adrena_abi::instruction::ClosePositionLong,
    adrena_abi::accounts::ClosePositionLong,
) {
    let args = adrena_abi::instruction::ClosePositionLong {
        params: adrena_abi::types::ClosePositionLongParams {
            price: if limit_price != 0 {
                Some(limit_price)
            } else {
                None
            },
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
        lm_staking_reward_token_vault: adrena_abi::pda::get_staking_reward_token_vault_pda(&lm_staking).0,
        lp_staking_reward_token_vault: adrena_abi::pda::get_staking_reward_token_vault_pda(&lp_staking).0,
        lp_token_mint: ALP_MINT,
        protocol_fee_recipient: cortex.protocol_fee_recipient,
        token_program: SPL_TOKEN_PROGRAM_ID,
        adrena_program: adrena_abi::ID,
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
            price: if limit_price != 0 {
                Some(limit_price)
            } else {
                None
            },
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
        lm_staking_reward_token_vault: adrena_abi::pda::get_staking_reward_token_vault_pda(&lm_staking).0,
        lp_staking_reward_token_vault: adrena_abi::pda::get_staking_reward_token_vault_pda(&lp_staking).0,
        lp_token_mint: ALP_MINT,
        protocol_fee_recipient: cortex.protocol_fee_recipient,
        token_program: SPL_TOKEN_PROGRAM_ID,
        adrena_program: adrena_abi::ID,
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
    staking_reward_token_custody: &adrena_abi::types::Custody,
    custody: &adrena_abi::types::Custody,
) -> (adrena_abi::instruction::LiquidateLong, adrena_abi::accounts::LiquidateLong) {
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
        lm_staking_reward_token_vault: adrena_abi::pda::get_staking_reward_token_vault_pda(&lm_staking).0,
        lp_staking_reward_token_vault: adrena_abi::pda::get_staking_reward_token_vault_pda(&lp_staking).0,
        lp_token_mint: ALP_MINT,
        protocol_fee_recipient: cortex.protocol_fee_recipient,
        token_program: SPL_TOKEN_PROGRAM_ID,
        adrena_program: adrena_abi::ID,
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
    staking_reward_token_custody: &adrena_abi::types::Custody,
    custody: &adrena_abi::types::Custody,
    collateral_custody: &adrena_abi::types::Custody,
) -> (adrena_abi::instruction::LiquidateShort, adrena_abi::accounts::LiquidateShort) {
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
        lm_staking_reward_token_vault: adrena_abi::pda::get_staking_reward_token_vault_pda(&lm_staking).0,
        lp_staking_reward_token_vault: adrena_abi::pda::get_staking_reward_token_vault_pda(&lp_staking).0,
        lp_token_mint: ALP_MINT,
        protocol_fee_recipient: cortex.protocol_fee_recipient,
        token_program: SPL_TOKEN_PROGRAM_ID,
        adrena_program: adrena_abi::ID,
    };
    (args, accounts)
}

pub fn create_execute_limit_order_long_ix(
    payer: &Pubkey,
    owner: &Pubkey,
    position_key: &Pubkey,
    pool: &Pubkey,
    transfer_authority_pda: &Pubkey,
    collateral_escrow: &Pubkey,
    limit_order_book: &Pubkey,
    custody: &Pubkey,
    custody_account: &adrena_abi::types::Custody,
    id: u64,
) -> (
    adrena_abi::instruction::ExecuteLimitOrderLong,
    adrena_abi::accounts::ExecuteLimitOrderLong,
) {
    let args = adrena_abi::instruction::ExecuteLimitOrderLong {
        params: adrena_abi::types::ExecuteLimitOrderLongParams { id },
    };
    let accounts = adrena_abi::accounts::ExecuteLimitOrderLong {
        transfer_authority: *transfer_authority_pda,
        cortex: CORTEX_ID,
        pool: *pool,
        position: *position_key,
        custody: *custody,
        custody_oracle: custody_account.oracle,
        custody_trade_oracle: custody_account.trade_oracle,
        custody_token_account: custody_account.token_account,
        token_program: SPL_TOKEN_PROGRAM_ID,
        adrena_program: adrena_abi::ID,
        owner: *owner,
        caller: *payer,
        collateral_escrow: *collateral_escrow,
        limit_order_book: *limit_order_book,
        system_program: SYSTEM_PROGRAM_ID,
    };
    (args, accounts)
}

pub fn create_execute_limit_order_short_ix(
    payer: &Pubkey,
    owner: &Pubkey,
    position_key: &Pubkey,
    pool: &Pubkey,
    transfer_authority_pda: &Pubkey,
    collateral_escrow: &Pubkey,
    limit_order_book: &Pubkey,
    custody: &Pubkey,
    collateral_custody: &Pubkey,
    custody_account: &adrena_abi::types::Custody,
    collateral_custody_account: &adrena_abi::types::Custody,
    id: u64,
) -> (
    adrena_abi::instruction::ExecuteLimitOrderShort,
    adrena_abi::accounts::ExecuteLimitOrderShort,
) {
    let args = adrena_abi::instruction::ExecuteLimitOrderShort {
        params: adrena_abi::types::ExecuteLimitOrderShortParams { id },
    };
    let accounts = adrena_abi::accounts::ExecuteLimitOrderShort {
        transfer_authority: *transfer_authority_pda,
        cortex: CORTEX_ID,
        pool: *pool,
        position: *position_key,
        custody: *custody,
        collateral_custody_oracle: collateral_custody_account.oracle,
        custody_trade_oracle: custody_account.trade_oracle,
        collateral_custody_token_account: collateral_custody_account.token_account,
        collateral_custody: *collateral_custody,
        token_program: SPL_TOKEN_PROGRAM_ID,
        adrena_program: adrena_abi::ID,
        owner: *owner,
        caller: *payer,
        collateral_escrow: *collateral_escrow,
        limit_order_book: *limit_order_book,
        system_program: SYSTEM_PROGRAM_ID,
    };
    (args, accounts)
}
