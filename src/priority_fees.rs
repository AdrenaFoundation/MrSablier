use {
    adrena_abi::ADRENA_PROGRAM_ID,
    anchor_client::Client,
    serde_json,
    solana_client::rpc_response::RpcPrioritizationFee,
    solana_sdk::{pubkey::Pubkey, signature::Keypair},
    std::{error::Error, sync::Arc},
};

pub struct GetRecentPrioritizationFeesByPercentileConfig {
    pub percentile: Option<u64>,
    pub fallback: bool,
    pub locked_writable_accounts: Vec<Pubkey>,
}

pub async fn fetch_mean_priority_fee(
    client: &Client<Arc<Keypair>>,
    percentile: u64,
) -> Result<u64, anyhow::Error> {
    let config = GetRecentPrioritizationFeesByPercentileConfig {
        percentile: Some(percentile),
        fallback: false,
        locked_writable_accounts: vec![], //adrena_abi::MAIN_POOL_ID, adrena_abi::CORTEX_ID],
    };
    get_mean_prioritization_fee_by_percentile(client, &config, None)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to fetch mean priority fee: {:?}", e))
}

pub async fn get_recent_prioritization_fees_by_percentile(
    client: &Client<Arc<Keypair>>,
    config: &GetRecentPrioritizationFeesByPercentileConfig,
    slots_to_return: Option<usize>,
) -> Result<Vec<RpcPrioritizationFee>, Box<dyn Error>> {
    let accounts: Vec<String> = config
        .locked_writable_accounts
        .iter()
        .map(|key| key.to_string())
        .collect();
    let mut args = vec![serde_json::to_value(accounts)?];

    if let Some(percentile) = config.percentile {
        args.push(serde_json::to_value(vec![percentile])?);
    }

    let response: Vec<RpcPrioritizationFee> = client
        .program(ADRENA_PROGRAM_ID)?
        .rpc()
        .send(
            solana_client::rpc_request::RpcRequest::GetRecentPrioritizationFees,
            serde_json::Value::from(args),
        )
        .await?;

    let mut recent_prioritization_fees: Vec<RpcPrioritizationFee> = response;

    recent_prioritization_fees.sort_by_key(|fee| fee.slot);

    if let Some(slots) = slots_to_return {
        recent_prioritization_fees.truncate(slots);
    }

    Ok(recent_prioritization_fees)
}

pub async fn get_mean_prioritization_fee_by_percentile(
    client: &Client<Arc<Keypair>>,
    config: &GetRecentPrioritizationFeesByPercentileConfig,
    slots_to_return: Option<usize>,
) -> Result<u64, Box<dyn Error>> {
    let recent_prioritization_fees =
        get_recent_prioritization_fees_by_percentile(client, config, slots_to_return).await?;

    if recent_prioritization_fees.is_empty() {
        return Err("No prioritization fees retrieved".into());
    }

    let sum: u64 = recent_prioritization_fees
        .iter()
        .map(|fee| fee.prioritization_fee)
        .sum();

    let mean = (sum + recent_prioritization_fees.len() as u64 - 1)
        / recent_prioritization_fees.len() as u64;

    Ok(mean)
}
