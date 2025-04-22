use {
    adrena_abi::{limited_string::LimitedString, oracle::OraclePrice},
    reqwest,
    serde::{Deserialize, Serialize},
    std::collections::HashMap,
};

#[derive(Debug, Serialize, Deserialize)]
struct LastTradingPricesResponse {
    pub success: bool,
    pub data: LastTradingPricesData,
}

#[derive(Debug, Serialize, Deserialize)]
struct LastTradingPricesData {
    pub latest_timestamp: String,
    pub solusd_price: String,
    pub jitosolusd_price: String,
    pub btcusd_price: String,
    pub wbtcusd_price: String,
    pub bonkusd_price: String,
    pub usdcusd_price: String,
    pub solusd_price_ts: String,
    pub jitosolusd_price_ts: String,
    pub btcusd_price_ts: String,
    pub wbtcusd_price_ts: String,
    pub bonkusd_price_ts: String,
    pub usdcusd_price_ts: String,
    pub signature: String,
    pub recovery_id: u8,
}

pub async fn get_last_trading_prices() -> Result<HashMap<LimitedString, OraclePrice>, backoff::Error<anyhow::Error>> {
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

    let last_trading_prices: LastTradingPricesResponse = serde_json::from_str(&body)
        .map_err(|e| backoff::Error::Permanent(anyhow::anyhow!("Failed to parse last trading prices: {}", e)))?;

    Ok(parse_last_trading_prices(last_trading_prices))
}

fn parse_last_trading_prices(response: LastTradingPricesResponse) -> HashMap<LimitedString, OraclePrice> {
    let mut prices = HashMap::new();

    let pairs = [
        ("SOL/USD", &response.data.solusd_price, &response.data.solusd_price_ts),
        (
            "JITOSOL/USD",
            &response.data.jitosolusd_price,
            &response.data.jitosolusd_price_ts,
        ),
        ("BTC/USD", &response.data.btcusd_price, &response.data.btcusd_price_ts),
        ("WBTC/USD", &response.data.wbtcusd_price, &response.data.wbtcusd_price_ts),
        ("BONK/USD", &response.data.bonkusd_price, &response.data.bonkusd_price_ts),
        ("USDC/USD", &response.data.usdcusd_price, &response.data.usdcusd_price_ts),
    ];

    for (ticker, price, timestamp) in pairs {
        if let (Ok(price), Ok(timestamp)) = (price.parse::<u64>(), timestamp.parse::<i64>()) {
            prices.insert(LimitedString::new(ticker), OraclePrice::new(price, -6, timestamp as u64));
        }
    }

    prices
}
