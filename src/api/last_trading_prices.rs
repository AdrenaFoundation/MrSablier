use {
    adrena_abi::{
        limited_string::LimitedString,
        oracle::{ChaosLabsBatchPrices, OraclePrice, PriceData},
    },
    hex::FromHex,
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

    Ok((
        parse_last_trading_prices(&response),
        parse_chaos_labs_batch_prices(&response),
    ))
}

fn parse_last_trading_prices(response: &LastTradingPricesResponse) -> HashMap<LimitedString, OraclePrice> {
    let mut prices = HashMap::new();

    let pairs = [
        ("solusd", &response.data.solusd_price, &response.data.solusd_price_ts),
        (
            "jitosolusd",
            &response.data.jitosolusd_price,
            &response.data.jitosolusd_price_ts,
        ),
        ("btcusd", &response.data.btcusd_price, &response.data.btcusd_price_ts),
        ("wbtcusd", &response.data.wbtcusd_price, &response.data.wbtcusd_price_ts),
        ("bonkusd", &response.data.bonkusd_price, &response.data.bonkusd_price_ts),
        ("usdcusd", &response.data.usdcusd_price, &response.data.usdcusd_price_ts),
    ];

    for (ticker, price, timestamp) in pairs {
        if let (Ok(price), Ok(timestamp)) = (price.parse::<u64>(), timestamp.parse::<i64>()) {
            // All chaos lab prices are exponent 10
            prices.insert(LimitedString::new(ticker), OraclePrice::new(price, -10, timestamp as u64));
        }
    }

    prices
}

fn parse_chaos_labs_batch_prices(response: &LastTradingPricesResponse) -> ChaosLabsBatchPrices {
    let mut prices = vec![];
    prices.push(PriceData {
        feed_id: 0,
        price: response.data.solusd_price.parse::<u64>().unwrap(),
        timestamp: response.data.solusd_price_ts.parse::<i64>().unwrap(),
    });
    prices.push(PriceData {
        feed_id: 1,
        price: response.data.jitosolusd_price.parse::<u64>().unwrap(),
        timestamp: response.data.jitosolusd_price_ts.parse::<i64>().unwrap(),
    });
    prices.push(PriceData {
        feed_id: 2,
        price: response.data.btcusd_price.parse::<u64>().unwrap(),
        timestamp: response.data.btcusd_price_ts.parse::<i64>().unwrap(),
    });
    prices.push(PriceData {
        feed_id: 3,
        price: response.data.wbtcusd_price.parse::<u64>().unwrap(),
        timestamp: response.data.wbtcusd_price_ts.parse::<i64>().unwrap(),
    });
    prices.push(PriceData {
        feed_id: 4,
        price: response.data.bonkusd_price.parse::<u64>().unwrap(),
        timestamp: response.data.bonkusd_price_ts.parse::<i64>().unwrap(),
    });
    prices.push(PriceData {
        feed_id: 5,
        price: response.data.usdcusd_price.parse::<u64>().unwrap(),
        timestamp: response.data.usdcusd_price_ts.parse::<i64>().unwrap(),
    });

    let signature_vec: [u8; 64] = {
        let vec = Vec::from_hex(&response.data.signature).unwrap();
        vec.try_into().expect("Hex string has incorrect length")
    };

    let batch_prices = ChaosLabsBatchPrices {
        prices,
        signature: signature_vec,
        recovery_id: response.data.recovery_id,
    };

    batch_prices
}
