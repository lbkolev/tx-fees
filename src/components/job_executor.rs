use std::collections::{HashMap, HashSet};

use alloy::{
    eips::BlockId,
    providers::{Provider, RootProvider},
    pubsub::PubSubFrontend,
    rpc::types::{BlockTransactionsKind, Filter},
};
use eyre::Result;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::{
    configs::JobExecutorConfig,
    helpers::{store_block, store_tx},
    price_providers::{get_pair_price, Binance},
};

/// Find the closest block to the given `target_timestamp`,
/// given average block production time, block number and timestamp
fn avg_ts_to_block_number(
    avg_block_time: i8,
    target_ts: i64,
    src_ts: i64,
    src_block_num: u64,
) -> u64 {
    let block_num_diff = (src_ts - target_ts) / avg_block_time as i64;

    if block_num_diff >= 0 {
        src_block_num.saturating_sub(block_num_diff as u64)
    } else {
        src_block_num.saturating_add(block_num_diff.unsigned_abs())
    }
}

async fn find_closest_block(
    provider: &RootProvider<PubSubFrontend>,
    target_ts: i64,
) -> Result<u64> {
    const AVG_BLOCK_TIME: i8 = 12;

    let latest_block = provider
        .get_block(BlockId::latest(), BlockTransactionsKind::Hashes)
        .await?
        .unwrap();
    info!(
        "Latest block: number={}, timestamp={}",
        latest_block.header.number, latest_block.header.timestamp
    );

    let mut estimated_block = avg_ts_to_block_number(
        AVG_BLOCK_TIME, // avg block time
        target_ts,
        latest_block.header.timestamp as i64,
        latest_block.header.number,
    );
    info!("target block estimate: {}", estimated_block);

    loop {
        /*
            1. Return the block if we find an exact match
            2. Return the closest block whose timestamp is less than or equal to the target when an exact match isn't found
            3. Compare with the next block to ensure we're at the closest position
        */
        let current_block = provider
            .get_block(estimated_block.into(), BlockTransactionsKind::Hashes)
            .await?
            .unwrap();
        let current_ts = current_block.header.timestamp as i64;
        info!(
            "Checking block {} with timestamp {}, target_ts={}",
            estimated_block, current_ts, target_ts
        );

        if target_ts == current_ts {
            return Ok(current_block.header.number);
        }

        // check next block to determine if we're at the closest block before target
        let next_block = provider
            .get_block((estimated_block + 1).into(), BlockTransactionsKind::Hashes)
            .await?
            .unwrap();
        let next_ts = next_block.header.timestamp as i64;

        if target_ts >= current_ts && target_ts <= next_ts {
            // return the closer block by comparing time differences
            let diff_to_current = target_ts - current_ts;
            let diff_to_next = next_ts - target_ts;

            return Ok(if diff_to_current < diff_to_next {
                current_block.header.number
            } else {
                next_block.header.number
            });
        }

        if target_ts < current_ts && (current_ts - target_ts) < AVG_BLOCK_TIME as i64 {
            let prev_block = provider
                .get_block((estimated_block - 1).into(), BlockTransactionsKind::Hashes)
                .await?
                .unwrap();

            if target_ts >= prev_block.header.timestamp as i64 {
                return Ok(prev_block.header.number);
            } else {
                estimated_block = avg_ts_to_block_number(
                    AVG_BLOCK_TIME,
                    target_ts,
                    current_ts,
                    current_block.header.number,
                );
            }
        } else {
            estimated_block = avg_ts_to_block_number(
                AVG_BLOCK_TIME,
                target_ts,
                next_ts,
                next_block.header.number,
            );
        }
    }
}

/*
 * Get all events in the given block range
 * 1. receives a filter with start and end block numbers, and the pool address
 * 2. retrieves all logs in the range
 * 3. filters unique transactions
 * 4. retrieves transaction receipts
 * 5. groups transactions by block number
 * 6. retrieves block details
 * 7. calculates transaction fees
 *
 * there's quite a lot of room for improvement here, for example
 * - we can first check if a similar job has been processed before
 * - we can batch process transactions in a single block
 * - when the range is too large, we can split it into smaller chunks because the provider might not return all logs at once
 * - we can bulk insert the data into the database
 */
async fn get_events_range(
    provider: &RootProvider<PubSubFrontend>,
    filter: Filter,
) -> Result<HashMap<(u64, i64), Vec<(String, u128, u64)>>> {
    info!("Starting get_events_range with filter: {:?}", filter);
    let mut events_by_block = HashMap::new();

    let logs = provider.get_logs(&filter).await?;
    info!("Received {} logs from filter", logs.len());

    let unique_txs: HashSet<_> = logs.iter().filter_map(|log| log.transaction_hash).collect();
    info!("Processing {} unique transactions", unique_txs.len());

    for tx_hash in unique_txs {
        let receipt = provider.get_transaction_receipt(tx_hash).await?.unwrap();
        let block_num = receipt.block_number.unwrap();
        events_by_block
            .entry(block_num)
            .or_insert_with(Vec::new)
            .push((
                tx_hash.to_string(),
                receipt.effective_gas_price,
                receipt.gas_used,
            ));
    }

    let mut final_events = HashMap::new();
    for (block_num, txs) in events_by_block {
        let block = provider
            .get_block(block_num.into(), BlockTransactionsKind::Hashes)
            .await?
            .unwrap();
        final_events.insert((block_num, block.header.timestamp as i64), txs);
    }

    Ok(final_events)
}

#[derive(Debug, Serialize, Deserialize)]
struct BatchJob {
    id: i64,
    start_time: i64,
    end_time: i64,
    start_block: Option<i64>,
    end_block: Option<i64>,
    status: String,
}

/*
 * The main component that listens for new jobs in the Redis queue
 * and processes them one by one. Each job process goes through the following steps:
 * 1. Receive a new job from the redis queue
 * 2. Update the job status to 'processing' in the database
 * 3. Find the closest block numbers to the start and end timestamps
 * 4. Retrieve all events in the block range
 * 5. Calculate transaction fees for each transaction
 * 6. Store the block and transaction details in the database
 *
*/
pub struct JobExecutorApp;
impl JobExecutorApp {
    pub async fn run(config: JobExecutorConfig) -> Result<()> {
        let mut con = config
            .redis_client
            .get_multiplexed_async_connection()
            .await?;
        let price_provider = Binance::new("ETHUSDT");
        let provider = config.provider.clone();

        loop {
            // BRPOP returns (key, value) tuple as strings
            info!("Waiting for new jobs...");
            let result: Option<(String, String)> = con.brpop("batch_jobs", 0.0).await?;

            if let Some((_, job_id_str)) = result {
                let job_id: i64 = job_id_str.parse()?;

                let job: BatchJob =
                    sqlx::query_as!(BatchJob, "SELECT id, start_time, end_time, start_block, end_block, status FROM batch_jobs WHERE id = $1", job_id)
                        .fetch_one(&config.db_pool)
                        .await?;
                info!(
                    "Job details - start_time: {}, end_time: {}, status: {}",
                    job.start_time, job.end_time, job.status
                );

                if job.status != "pending" {
                    info!("Ignoring job {} with status {}", job.id, job.status);
                    continue;
                }

                sqlx::query!(
                    "UPDATE batch_jobs SET status = 'processing', updated_at = NOW() WHERE id = $1",
                    job_id
                )
                .execute(&config.db_pool)
                .await?;
                info!("Processing job {}", job_id);

                let start_block = find_closest_block(&provider, job.start_time).await?;
                let end_block = find_closest_block(&provider, job.end_time).await?;
                info!(
                    "Block range found - start: {}, end: {}",
                    start_block, end_block
                );

                sqlx::query!(
                    "UPDATE batch_jobs SET start_block = $1, end_block = $2 WHERE id = $3",
                    start_block as i64,
                    end_block as i64,
                    job_id
                )
                .execute(&config.db_pool)
                .await?;

                let filter = Filter::new()
                    .from_block(start_block)
                    .to_block(end_block)
                    .address(config.pool_address);
                let events = get_events_range(&provider, filter).await?;
                info!("Found {} blocks with events", events.len());

                for ((block_num, _), txs) in events {
                    info!(
                        "Processing block {} with {} transactions",
                        block_num,
                        txs.len(),
                    );

                    let block = provider
                        .get_block(BlockId::number(block_num), BlockTransactionsKind::Hashes)
                        .await?
                        .unwrap();

                    let eth_price =
                        get_pair_price(&price_provider, Some(block.header.timestamp as i64))
                            .await?;

                    let mut tx_fees = Vec::new();

                    for (tx_hash, gas_price, gas_used) in txs {
                        let fee_eth = (gas_price as f64 * gas_used as f64) / 1e18;
                        let fee_usdt = fee_eth * eth_price;

                        tx_fees.push((tx_hash, fee_usdt));
                    }

                    store_block(
                        &config.db_pool,
                        block_num as i64,
                        &block.header.hash.to_string(),
                        eth_price,
                    )
                    .await?;

                    for (tx_hash, fee_usdt) in tx_fees {
                        store_tx(
                            &config.db_pool,
                            &tx_hash,
                            &block.header.hash.to_string(),
                            fee_usdt,
                        )
                        .await?;
                    }
                }

                sqlx::query!(
                    "UPDATE batch_jobs SET status = 'completed', updated_at = NOW() WHERE id = $1",
                    job_id
                )
                .execute(&config.db_pool)
                .await?;

                info!("Completed job {}", job_id);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy::{primitives::Address, providers::ProviderBuilder};
    use std::{env::var, str::FromStr};

    #[tokio::test]
    async fn test_avg_closest_block() {
        // assume avg block time of 12s
        let avg_time = 12;

        let test_cases = vec![
            // 1 hour difference = 3600 seconds = 300 blocks
            (
                17_000_000,       // block_num
                1_697_890_000,    // block_ts
                1_697_886_400,    // target_ts (1h earlier)
                17_000_000 - 300, // expected: 16999700
            ),
            // 24 hours = 86400 seconds = 7200 blocks
            (
                17_000_000,
                1_697_890_000,     // block_ts
                1_697_803_600,     // target_ts (24h earlier)
                17_000_000 - 7200, // expected: 16992800
            ),
            // 5 minutes = 300 seconds = 25 blocks
            (
                17_000_000,
                1_697_890_000,   // block_ts
                1_697_889_700,   // target_ts (5min earlier)
                17_000_000 - 25, // expected: 16999975
            ),
            (
                17_000_000,
                1_697_890_000,
                1_697_889_988,  // 12 seconds earlier
                17_000_000 - 1, // expected: one block difference
            ),
            // less than avg_time
            (
                17_000_000, 1697890000, 1697889995, // only 5 seconds difference
                17_000_000, // expected: same block (diff truncates to 0)
            ),
            // exactly avg_time
            (
                17_000_000,
                1_697_890_000,
                1_697_889_988,  // exactly 12 seconds
                17_000_000 - 1, // expected: one block difference
            ),
        ];

        for (block_num, block_ts, target_ts, expected) in test_cases {
            let result = avg_ts_to_block_number(avg_time, target_ts, block_ts, block_num);
            assert_eq!(result, expected);
        }
    }

    async fn setup_provider(rpc_url: Option<&str>) -> RootProvider<PubSubFrontend> {
        let ws = if let Some(url) = rpc_url {
            alloy::providers::WsConnect::new(url)
        } else {
            alloy::providers::WsConnect::new(
                &var("TEST_ETH_WS_RPC_URL")
                    .unwrap_or_else(|_| "wss://mainnet.gateway.tenderly.co/".to_string()),
            )
        };
        ProviderBuilder::new().on_ws(ws).await.unwrap()
    }

    #[tokio::test]
    async fn test_find_closest_block() {
        let provider = setup_provider(None).await;

        let test_cases = vec![
            //   (target timestmap, expected block, error message)
            (1471758485, 2111111, "Historical block from 2016"),
            (1438269988, 1, "Near genesis block"),
            (1730685059, 21111111, "Standard case - exact block match"),
            (1738435775, 21753567, "Exact block timestamp match"),
            (1730685065, 21111111, "Timestamp slightly after block"), // real block timestamp: 1730685059
            (1730685055, 21111110, "Timestamp slightly before block"), // real block timestamp: 1730685047
        ];

        for (timestamp, expected_block, error_msg) in test_cases {
            let result = find_closest_block(&provider, timestamp).await.unwrap();
            assert_eq!(result, expected_block, "{}", error_msg);
        }
    }

    #[tokio::test]
    async fn test_get_events_range() {
        let provider = setup_provider(None).await;

        let start_block = 17000000;
        let end_block = 17000100;
        let pool_address = Address::from_str(
            &var("TEST_POOL_ADDRESS")
                .unwrap_or_else(|_| "0x88e6a0c2ddd26feeb64f039a2c41296fcb3f5640".to_string()),
        )
        .expect("Invalid address format");

        let filter = Filter::new()
            .from_block(start_block)
            .to_block(end_block)
            .address(pool_address);

        let events = get_events_range(&provider, filter).await.unwrap();

        assert!(!events.is_empty(), "Should find some events");

        for ((block_num, timestamp), txs) in events {
            assert!(block_num >= start_block && block_num <= end_block);
            assert!(timestamp > 0, "Timestamp should be positive");

            for (tx_hash, gas_price, gas_used) in txs {
                assert!(tx_hash.starts_with("0x") && tx_hash.len() == 66);
                assert!(gas_price > 0, "Gas price should be non-zero");
                assert!(gas_used > 0, "Gas used should be non-zero");
            }
        }
    }
}
