use alloy::{
    primitives::{address, Address},
    providers::{Provider, ProviderBuilder, WsConnect},
    rpc::types::{BlockNumberOrTag, Filter},
};
use eyre::Result;
use futures_util::stream::StreamExt;
use reqwest;
use serde_json::Value;
use sqlx::PgPool;
use tracing::info;

use std::collections::{HashMap, HashSet};

// uniswapV3 pool
const ETH_USDC_POOL: Address = address!("0x88e6a0c2ddd26feeb64f039a2c41296fcb3f5640");

pub async fn realtime(db_pool: &PgPool, rpc_url: &String) -> Result<()> {
    let ws = WsConnect::new(rpc_url);
    let provider = ProviderBuilder::new().on_ws(ws).await?;

    let filter = Filter::new()
        .address(ETH_USDC_POOL)
        .from_block(BlockNumberOrTag::Latest);

    let sub = provider.subscribe_logs(&filter).await?;
    let mut stream = sub.into_stream();
    let mut seen_txs = HashSet::new();
    let mut seen_blocks: HashMap<String, f64> = HashMap::new(); // block_hash -> eth_usdt price

    while let Some(log) = stream.next().await {
        if let Some(tx_hash) = log.transaction_hash {
            if seen_txs.insert(tx_hash) {
                if let Some(receipt) = provider.get_transaction_receipt(tx_hash).await? {
                    let block_hash = receipt.block_hash.expect("No block hash").to_string();
                    let block_number = receipt.block_number.expect("No block number") as i64;

                    let eth_usdt = if !seen_blocks.contains_key(&block_hash) {
                        let price = get_ethusdt_price().await?;
                        store_block(db_pool, block_number, &block_hash, price).await?;
                        seen_blocks.insert(block_hash.clone(), price);
                        price
                    } else {
                        *seen_blocks.get(&block_hash).unwrap()
                    };

                    let transaction_fee_usdt = receipt.effective_gas_price as f64
                        * receipt.gas_used as f64
                        * 1e-18
                        * eth_usdt;
                    store_tx(
                        db_pool,
                        &tx_hash.to_string(),
                        &block_hash,
                        transaction_fee_usdt,
                    )
                    .await?;

                    info!(
                        tx_hash = ?tx_hash,
                        eth_usdt = eth_usdt,
                        effective_gas_price = ?receipt.effective_gas_price,
                        gas_used = ?receipt.gas_used,
                        fee = receipt.effective_gas_price as f64 * receipt.gas_used as f64 * 1e-18,
                        fee_usdt = transaction_fee_usdt,
                        "new tx |"
                    );
                }
            }
        }
    }
    Ok(())
}

async fn get_ethusdt_price() -> Result<f64> {
    let url = "https://api.binance.com/api/v3/ticker/price?symbol=ETHUSDT";

    let response = reqwest::get(url).await?.text().await?;
    let json: Value = serde_json::from_str(&response)?;

    json["price"]
        .as_str()
        .ok_or(eyre::eyre!("Failed to get price as string"))?
        .parse::<f64>()
        .map_err(|e| eyre::eyre!("Failed to parse price: {}", e))
}

async fn store_tx(pool: &PgPool, tx_hash: &str, block_hash: &str, fee_usdt: f64) -> Result<()> {
    sqlx::query!(
        "INSERT INTO txs (hash, block_hash, fee_usdt) VALUES ($1, $2, $3)",
        tx_hash,
        block_hash,
        fee_usdt
    )
    .execute(pool)
    .await?;
    Ok(())
}

async fn store_block(
    pool: &PgPool,
    block_number: i64,
    block_hash: &str,
    eth_usdt: f64,
) -> Result<()> {
    sqlx::query!(
        "INSERT INTO blocks (hash, number, eth_usdt) VALUES ($1, $2, $3)",
        block_hash,
        block_number,
        eth_usdt
    )
    .execute(pool)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_ethusdt_price_format() {
        let price = get_ethusdt_price().await.unwrap();

        assert!(price > 0.0);
        assert!(price < 100000.0); // sanity check
    }
}
