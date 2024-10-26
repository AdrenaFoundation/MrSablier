use {
    solana_client::{nonblocking::rpc_client::RpcClient, rpc_response::RpcPrioritizationFee},
    solana_sdk::pubkey::Pubkey,
    std::error::Error,
};

pub struct GetRecentPrioritizationFeesByPercentileConfig {
    pub percentile: Option<u64>,
    pub fallback: bool,
    pub locked_writable_accounts: Vec<Pubkey>,
}

pub async fn get_recent_prioritization_fees_by_percentile(
    client: &RpcClient,
    config: &GetRecentPrioritizationFeesByPercentileConfig,
    slots_to_return: Option<usize>,
) -> Result<Vec<RpcPrioritizationFee>, Box<dyn Error>> {
    let accounts: Vec<String> = config
        .locked_writable_accounts
        .iter()
        .map(|key| key.to_string())
        .collect();
    let mut args = vec![accounts];

    if let Some(percentile) = config.percentile {
        args.push(vec![percentile.to_string()]);
    }

    let response: Vec<RpcPrioritizationFee> = client
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

pub async fn get_median_prioritization_fee_by_percentile(
    client: &RpcClient,
    config: &GetRecentPrioritizationFeesByPercentileConfig,
    slots_to_return: Option<usize>,
) -> Result<u64, Box<dyn Error>> {
    let mut recent_prioritization_fees =
        get_recent_prioritization_fees_by_percentile(client, config, slots_to_return).await?;

    recent_prioritization_fees.sort_by_key(|fee| fee.prioritization_fee);

    let half = recent_prioritization_fees.len() / 2;

    let median = if recent_prioritization_fees.len() % 2 == 0 {
        (recent_prioritization_fees[half - 1].prioritization_fee
            + recent_prioritization_fees[half].prioritization_fee
            + 1)
            / 2
    } else {
        recent_prioritization_fees[half].prioritization_fee
    };

    Ok(median)
}
