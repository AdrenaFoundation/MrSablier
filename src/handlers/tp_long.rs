use {
    crate::{utils, IndexedCustodiesThreadSafe},
    adrena_abi::{
        main_pool::USDC_CUSTODY_ID, types::Cortex, ADX_MINT, ALP_MINT, SABLIER_THREAD_PROGRAM_ID,
        SPL_ASSOCIATED_TOKEN_PROGRAM_ID, SPL_TOKEN_PROGRAM_ID, USDC_MINT,
    },
    solana_sdk::{pubkey::Pubkey, signature::Keypair},
    std::{collections::HashMap, rc::Rc, sync::Arc},
    tokio::sync::RwLock,
};

pub async fn tp_long(
    position_key: &Pubkey,
    position: &adrena_abi::types::Position,
    oracle_price: &utils::oracle_price::OraclePrice,
    indexed_custodies: &IndexedCustodiesThreadSafe,
    program: &anchor_client::Program<Rc<Keypair>>,
    cortex: &Cortex,
) -> Result<(), backoff::Error<anyhow::Error>> {
    Ok(())
}
