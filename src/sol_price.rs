use {
    adrena_abi::oracle_price::OraclePrice,
    log,
    solana_client::nonblocking::rpc_client::RpcClient,
    solana_sdk::pubkey::Pubkey,
    std::{str::FromStr, sync::Arc, time::Duration},
    tokio::{sync::RwLock, task::JoinHandle, time::interval},
};

// SOL Pyth Price feed pubkey
pub const SOL_PYTH_PRICE_FEED: &str = "H6ARHf6YXhGYeQfUzQNGk6rDNnLBQKrenN712K4AQJEG";

// Function to start a background task that periodically fetches SOL price
pub fn start_sol_price_update_task(endpoint: String, sol_price: Arc<RwLock<f64>>) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = interval(Duration::from_secs(20));
        let rpc_client = RpcClient::new(endpoint);

        // Try to get initial price
        match fetch_sol_price(&rpc_client).await {
            Ok(price) => {
                let mut sol_price_write = sol_price.write().await;
                *sol_price_write = price;
                log::info!("Initial SOL price from chain: ${:.2}", price);
            }
            Err(e) => {
                log::error!("Failed to fetch initial SOL price: {}", e);
            }
        }

        loop {
            interval.tick().await;
            log::info!("Fetching SOL price...");

            match fetch_sol_price(&rpc_client).await {
                Ok(price) => {
                    let mut sol_price_write = sol_price.write().await;
                    *sol_price_write = price;
                    log::info!("Updated SOL price: ${:.2}", price);
                }
                Err(e) => {
                    log::error!("Failed to fetch SOL price: {}", e);
                    // Don't update the price on error to keep using the last known good price
                }
            }
        }
    })
}

// Fetch SOL price directly from the Pyth oracle on-chain
async fn fetch_sol_price(rpc_client: &RpcClient) -> Result<f64, anyhow::Error> {
    // Get the SOL Pyth price feed account
    let sol_price_pubkey = Pubkey::from_str(SOL_PYTH_PRICE_FEED)?;
    let sol_price_account = rpc_client.get_account(&sol_price_pubkey).await?;

    // Try to deserialize using the current method
    let result = try_deserialize_pyth_price(&sol_price_account.data).await;

    if let Ok(price) = result {
        return Ok(price);
    }

    // If the current method fails, try an alternative approach or return the error
    Err(anyhow::anyhow!("Failed to parse SOL price using all available methods"))
}

/*  let trade_oracle: PriceUpdateV2 =
    borsh::BorshDeserialize::deserialize(&mut &trade_oracle_data[8..]).map_err(|e| backoff::Error::transient(e.into()))?;

// TODO: Optimize this by not creating the OraclePrice struct from the price update v2 account but just using the price and conf directly
// Create an OraclePrice struct from the price update v2 account
let oracle_price: OraclePrice =
    OraclePrice::new_from_pyth_price_update_v2(&trade_oracle).map_err(backoff::Error::transient)?; */

// Try to deserialize the Pyth price data using the current method
async fn try_deserialize_pyth_price(data: &[u8]) -> Result<f64, anyhow::Error> {
    // First try the standard method
    let result = borsh::BorshDeserialize::deserialize(&mut &data[8..])
        .map_err(|e| anyhow::anyhow!("Failed to deserialize SOL price: {}", e))
        .and_then(|price_data| {
            OraclePrice::new_from_pyth_price_update_v2(&price_data)
                .map_err(|e| anyhow::anyhow!("Failed to parse SOL price: {}", e))
                .map(|oracle_price| oracle_price.price as f64)
        });

    if result.is_ok() {
        return result;
    }

    // If the standard method fails, log the error and try to extract the price directly
    // This is a temporary workaround until the proper struct definition is updated
    log::warn!("Standard Pyth price deserialization failed, attempting direct extraction");

    // The error suggests a variant tag issue, which might mean the format has changed
    // For now, return an error to indicate we need to update the deserialization code
    Err(anyhow::anyhow!("Pyth price format may have changed, needs investigation"))
}
