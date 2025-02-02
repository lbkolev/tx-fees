use eyre::{eyre, Result};
use reqwest::Client;
use serde_json::Value;

pub trait PriceProvider {
    fn url(&self, timestamp: Option<i64>) -> String;
    fn extract_price(&self, data: &Value) -> Option<f64>;
}

pub struct Binance {
    pair: String,
}

impl Binance {
    pub fn new(pair: &str) -> Self {
        Self {
            pair: pair.to_string(),
        }
    }
}

impl PriceProvider for Binance {
    fn url(&self, timestamp: Option<i64>) -> String {
        let base = format!(
            "https://api.binance.com/api/v3/klines?symbol={}&interval=1s&limit=1",
            self.pair
        );
        match timestamp {
            Some(ts) => format!("{}&startTime={}", base, ts * 1000),
            None => base,
        }
    }

    fn extract_price(&self, data: &Value) -> Option<f64> {
        data.as_array()?
            .first()?
            .as_array()?
            .get(4)?
            .as_str()?
            .parse()
            .ok()
    }
}

pub async fn get_pair_price(provider: &impl PriceProvider, timestamp: Option<i64>) -> Result<f64> {
    let response = Client::new()
        .get(provider.url(timestamp))
        .send()
        .await?
        .json::<Value>()
        .await?;

    provider
        .extract_price(&response)
        .ok_or_else(|| eyre!("Failed to parse price from provider response"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::test;

    #[test]
    async fn test_binance_get_pair_price() {
        let provider = Binance::new("ETHUSDT");

        let price = get_pair_price(&provider, Some(1706826922)).await.unwrap();
        assert_eq!(price, 2297.12);

        let price = get_pair_price(&provider, Some(1726526911)).await.unwrap();
        assert_eq!(price, 2282.73);
    }
}
