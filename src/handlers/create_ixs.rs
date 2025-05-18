use {
    adrena_abi::{oracle::ChaosLabsBatchPrices, CORTEX_ID, SPL_TOKEN_PROGRAM_ID, SYSTEM_PROGRAM_ID},
    solana_sdk::pubkey::Pubkey,
};

pub fn create_close_position_long_ix(
    payer: &Pubkey,
    position_key: &Pubkey,
    position: &adrena_abi::types::Position,
    receiving_account: Pubkey,
    transfer_authority_pda: Pubkey,
    user_profile: Option<Pubkey>,
    referrer_profile: Option<Pubkey>,
    custody: &adrena_abi::types::Custody,
    oracle_key: &Pubkey,
    oracle_prices: Option<ChaosLabsBatchPrices>,
    limit_price: u64,
) -> (
    adrena_abi::instruction::ClosePositionLong,
    adrena_abi::accounts::ClosePositionLong,
) {
    let args = adrena_abi::instruction::ClosePositionLong {
        params: adrena_abi::types::ClosePositionLongParams {
            price: if limit_price != 0 { Some(limit_price) } else { None },
            oracle_prices,
        },
    };
    let accounts = adrena_abi::accounts::ClosePositionLong {
        caller: *payer,
        owner: position.owner,
        receiving_account,
        transfer_authority: transfer_authority_pda,
        cortex: CORTEX_ID,
        pool: position.pool,
        position: *position_key,
        custody: position.custody,
        oracle: *oracle_key,
        custody_token_account: custody.token_account,
        user_profile,
        referrer_profile,
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
    user_profile: Option<Pubkey>,
    referrer_profile: Option<Pubkey>,
    collateral_custody: &adrena_abi::types::Custody,
    oracle_key: &Pubkey,
    oracle_prices: Option<ChaosLabsBatchPrices>,
    limit_price: u64,
) -> (
    adrena_abi::instruction::ClosePositionShort,
    adrena_abi::accounts::ClosePositionShort,
) {
    let args = adrena_abi::instruction::ClosePositionShort {
        params: adrena_abi::types::ClosePositionShortParams {
            price: if limit_price != 0 { Some(limit_price) } else { None },
            oracle_prices,
        },
    };
    let accounts = adrena_abi::accounts::ClosePositionShort {
        caller: *payer,
        owner: position.owner,
        receiving_account,
        transfer_authority: transfer_authority_pda,
        cortex: CORTEX_ID,
        pool: position.pool,
        position: *position_key,
        custody: position.custody,
        oracle: *oracle_key,
        collateral_custody: position.collateral_custody,
        collateral_custody_token_account: collateral_custody.token_account,
        user_profile,
        referrer_profile,
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
    custody: &adrena_abi::types::Custody,
    user_profile: Option<Pubkey>,
    referrer_profile: Option<Pubkey>,
    oracle_key: &Pubkey,
    oracle_prices: Option<ChaosLabsBatchPrices>,
) -> (adrena_abi::instruction::LiquidateLong, adrena_abi::accounts::LiquidateLong) {
    let args = adrena_abi::instruction::LiquidateLong {
        params: adrena_abi::types::LiquidateLongParams { oracle_prices },
    };
    let accounts = adrena_abi::accounts::LiquidateLong {
        signer: *payer,
        receiving_account,
        transfer_authority: transfer_authority_pda,
        cortex: CORTEX_ID,
        pool: position.pool,
        position: *position_key,
        custody: position.custody,
        oracle: *oracle_key,
        custody_token_account: custody.token_account,
        user_profile,
        referrer_profile,
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
    user_profile: Option<Pubkey>,
    referrer_profile: Option<Pubkey>,
    collateral_custody: &adrena_abi::types::Custody,
    oracle_key: &Pubkey,
    oracle_prices: Option<ChaosLabsBatchPrices>,
) -> (adrena_abi::instruction::LiquidateShort, adrena_abi::accounts::LiquidateShort) {
    let args = adrena_abi::instruction::LiquidateShort {
        params: adrena_abi::types::LiquidateShortParams { oracle_prices },
    };
    let accounts = adrena_abi::accounts::LiquidateShort {
        signer: *payer,
        receiving_account,
        transfer_authority: transfer_authority_pda,
        cortex: CORTEX_ID,
        pool: position.pool,
        position: *position_key,
        custody: position.custody,
        oracle: *oracle_key,
        collateral_custody: position.collateral_custody,
        collateral_custody_token_account: collateral_custody.token_account,
        user_profile,
        referrer_profile,
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
    oracle_key: &Pubkey,
    oracle_prices: Option<ChaosLabsBatchPrices>,
    id: u64,
) -> (
    adrena_abi::instruction::ExecuteLimitOrderLong,
    adrena_abi::accounts::ExecuteLimitOrderLong,
) {
    let args = adrena_abi::instruction::ExecuteLimitOrderLong {
        params: adrena_abi::types::ExecuteLimitOrderLongParams { id, oracle_prices },
    };
    let accounts = adrena_abi::accounts::ExecuteLimitOrderLong {
        transfer_authority: *transfer_authority_pda,
        cortex: CORTEX_ID,
        pool: *pool,
        position: *position_key,
        custody: *custody,
        oracle: *oracle_key,
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
    collateral_custody_account: &adrena_abi::types::Custody,
    oracle_key: &Pubkey,
    oracle_prices: Option<ChaosLabsBatchPrices>,
    id: u64,
) -> (
    adrena_abi::instruction::ExecuteLimitOrderShort,
    adrena_abi::accounts::ExecuteLimitOrderShort,
) {
    let args = adrena_abi::instruction::ExecuteLimitOrderShort {
        params: adrena_abi::types::ExecuteLimitOrderShortParams { id, oracle_prices },
    };
    let accounts = adrena_abi::accounts::ExecuteLimitOrderShort {
        transfer_authority: *transfer_authority_pda,
        cortex: CORTEX_ID,
        pool: *pool,
        position: *position_key,
        custody: *custody,
        oracle: *oracle_key,
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
