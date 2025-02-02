use std::collections::{HashMap, HashSet};

use alloy::{
    providers::Provider,
    rpc::types::{BlockNumberOrTag, Filter},
};
use eyre::Result;
use futures_util::stream::StreamExt;
use tracing::info;

use crate::{
    configs::FeeTrackerConfig,
    helpers::{calculate_tx_fee_usdt, store_block, store_tx},
    price_providers::{get_pair_price, Binance},
};

/*
 * Listens for new txs in the pool and calculates the fee in USDT
 * based on the effective gas price, gas used and the ETH/USDT price at the time the
 * block got confirmed. (i.e all txs in a block use the same ETH/USDT price)
 */
pub struct FeeTrackerApp;

impl FeeTrackerApp {
    pub async fn run(config: FeeTrackerConfig) -> Result<()> {
        let filter = Filter::new()
            .address(config.pool_address)
            .from_block(BlockNumberOrTag::Latest);

        let sub = config.provider.subscribe_logs(&filter).await?;

        let mut stream = sub.into_stream();
        let mut seen_txs = HashSet::new();
        let mut seen_blocks: HashMap<String, f64> = HashMap::new(); // block_hash -> eth_usdt price

        while let Some(log) = stream.next().await {
            if let Some(tx_hash) = log.transaction_hash {
                if seen_txs.insert(tx_hash) {
                    if let Some(receipt) = config.provider.get_transaction_receipt(tx_hash).await? {
                        let block_hash = receipt.block_hash.expect("No block hash").to_string();
                        let block_number = receipt.block_number.expect("No block number") as i64;

                        let eth_usdt = if !seen_blocks.contains_key(&block_hash) {
                            let price_provider = Binance::new(&config.price_pair);
                            let price = get_pair_price(&price_provider, None).await?;

                            store_block(&config.db_pool, block_number, &block_hash, price).await?;
                            seen_blocks.insert(block_hash.clone(), price);

                            price
                        } else {
                            *seen_blocks.get(&block_hash).unwrap()
                        };

                        let fee_usdt = calculate_tx_fee_usdt(
                            receipt.effective_gas_price,
                            receipt.gas_used,
                            eth_usdt,
                        );

                        store_tx(&config.db_pool, &tx_hash.to_string(), &block_hash, fee_usdt)
                            .await?;
                        info!(
                            tx_hash = ?tx_hash,
                            eth_usdt = eth_usdt,
                            effective_gas_price = ?receipt.effective_gas_price,
                            gas_used = ?receipt.gas_used,
                            fee = receipt.effective_gas_price as f64 * receipt.gas_used as f64 * 1e-18,
                            fee_usdt = fee_usdt,
                            "new tx |"
                        );
                    }
                }
            }
        }
        Ok(())
    }
}
