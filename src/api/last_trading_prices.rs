use {
    adrena_abi::{
        limited_string::LimitedString,
        oracle::{ChaosLabsBatchPrices, OraclePrice, PriceData},
    },
    hex::FromHex,
    reqwest,
    serde::{Deserialize, Serialize},
    serde_json::Number,
    std::collections::HashMap,
};

#[derive(Debug, Serialize, Deserialize)]
struct LastTradingPricesResponse {
    pub success: bool,
    pub data: LastTradingPricesData,
}

#[derive(Debug, Serialize, Deserialize)]
struct LastTradingPricesData {
    pub latest_date: String,
    pub latest_timestamp: Number,
    pub prices: Vec<TradingPriceData>,
    pub signature: String,
    pub recovery_id: u8,
}

#[derive(Debug, Serialize, Deserialize)]
struct TradingPriceData {
    pub symbol: String,
    pub feed_id: u8,
    pub price: Number,
    pub timestamp: Number,
    pub exponent: i8,
}

pub async fn get_last_trading_prices(
) -> Result<(HashMap<LimitedString, OraclePrice>, ChaosLabsBatchPrices), backoff::Error<anyhow::Error>> {
    let client = reqwest::Client::new();
    let response = client
        .get("https://datapi.adrena.xyz/last-trading-prices")
        .send()
        .await
        .map_err(|e| backoff::Error::Permanent(anyhow::anyhow!("Failed to send request: {}", e)))?;
    if !response.status().is_success() {
        return Err(backoff::Error::Permanent(anyhow::anyhow!(
            "Failed to fetch last trading prices"
        )));
    }

    let body = response
        .text()
        .await
        .map_err(|e| backoff::Error::Permanent(anyhow::anyhow!("Failed to get response body: {}", e)))?;

    let response: LastTradingPricesResponse = serde_json::from_str(&body)
        .map_err(|e| backoff::Error::Permanent(anyhow::anyhow!("Failed to parse last trading prices: {}", e)))?;
    Ok((parse_last_trading_prices(&response), parse_chaos_labs_batch_prices(&response)))
}

fn parse_last_trading_prices(response: &LastTradingPricesResponse) -> HashMap<LimitedString, OraclePrice> {
    let mut prices = HashMap::new();

    for price_data in response.data.prices.iter() {
        let price = price_data.price.as_u64().unwrap();
        let exponent = price_data.exponent as i32;
        let timestamp = price_data.timestamp.as_i64().unwrap();
        let name = LimitedString::new(price_data.symbol.clone());
        prices.insert(name.clone(), OraclePrice::new(price, exponent, 0, timestamp, &name));
    }
    prices
}

fn parse_chaos_labs_batch_prices(response: &LastTradingPricesResponse) -> ChaosLabsBatchPrices {
    let signature_vec: [u8; 64] = {
        let vec = Vec::from_hex(&response.data.signature).unwrap();
        vec.try_into().expect("Hex string has incorrect length")
    };
    let batch_prices = ChaosLabsBatchPrices {
        prices: response
            .data
            .prices
            .iter()
            .map(|price_data| PriceData {
                feed_id: price_data.feed_id,
                price: price_data.price.as_u64().unwrap(),
                timestamp: price_data.timestamp.as_i64().unwrap(),
            })
            .collect(),
        signature: signature_vec,
        recovery_id: response.data.recovery_id,
    };
    batch_prices
}
