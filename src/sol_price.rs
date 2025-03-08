use {
    adrena_abi::oracle_price::OraclePrice,
    log,
    solana_client::nonblocking::rpc_client::RpcClient,
    solana_sdk::pubkey::Pubkey,
    std::{sync::Arc, time::Duration},
    tokio::{sync::RwLock, task::JoinHandle, time::interval},
};

// Function to start a background task that periodically fetches SOL price
pub fn start_sol_price_update_task(endpoint: String, sol_price: Arc<RwLock<f64>>, sol_trade_oracle: Pubkey) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = interval(Duration::from_secs(20));
        let rpc_client = RpcClient::new(endpoint);

        // Try to get initial price
        match fetch_sol_price(&rpc_client, sol_trade_oracle).await {
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

            match fetch_sol_price(&rpc_client, sol_trade_oracle).await {
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
async fn fetch_sol_price(rpc_client: &RpcClient, sol_trade_oracle: Pubkey) -> Result<f64, anyhow::Error> {
    // Get the SOL Pyth price feed account
    let sol_price_account = rpc_client.get_account(&sol_trade_oracle).await?;

    // Try to deserialize using the current method
    let result = try_deserialize_pyth_price(&sol_price_account.data).await;

    if let Ok(price) = result {
        return Ok(price);
    }

    // If the current method fails, try an alternative approach or return the error
    Err(anyhow::anyhow!("Failed to parse SOL price using all available methods"))
}

// Try to deserialize the Pyth price data using the current method
async fn try_deserialize_pyth_price(data: &[u8]) -> Result<f64, anyhow::Error> {
    // First try the standard method
    let result = borsh::BorshDeserialize::deserialize(&mut &data[8..])
        .map_err(|e| anyhow::anyhow!("Failed to deserialize SOL price: {}", e))
        .and_then(|price_data| {
            OraclePrice::new_from_pyth_price_update_v2(&price_data)
                .map_err(|e| anyhow::anyhow!("Failed to parse SOL price: {}", e))
                .map(|oracle_price| {
                    // Apply the proper exponent here if needed
                    // This formula may need adjustment based on how the price is stored
                    let price = oracle_price.price as f64;
                    let exponent = oracle_price.exponent as i32;

                    if exponent < 0 {
                        price / 10f64.powi(-exponent)
                    } else {
                        price * 10f64.powi(exponent)
                    }
                })
        });

    if result.is_ok() {
        return result;
    }

    Err(anyhow::anyhow!(
        "Error happened during the deserialization of the SOL price, needs investigation"
    ))
}
