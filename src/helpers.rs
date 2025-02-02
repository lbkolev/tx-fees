use eyre::Result;
use sqlx::PgPool;

pub async fn store_tx(pool: &PgPool, tx_hash: &str, block_hash: &str, fee_usdt: f64) -> Result<()> {
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

pub async fn store_block(
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

/// Provides the transaction fee in USDT for a given gas price, gas used and ETH/USDT price
pub fn calculate_tx_fee_usdt(gas_price: u128, gas_used: u64, eth_usdt: f64) -> f64 {
    // 1e-18 is the conversion factor from wei to ETH
    (gas_price as f64 * gas_used as f64 * 1e-18) * eth_usdt
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_tx_fee_usdt() {
        let test_cases = vec![
            // (gas_price, gas_used, eth_usdt, expected_fee)
            (
                50_000_000_000, // 50 gwei
                21000,          // standard transfer
                2500.0,         // eth price
                2.625,          // expected fee in USDT
            ),
            (
                100_000_000_000, // 100 gwei
                100000,          //
                2000.0,          //
                20.0,            // expected fee
            ),
            (
                30_000_000_000, // 30 gwei
                300000,         //
                3000.0,         //
                27.0,           // expected fee
            ),
            (
                200_000_000_000, // 200 gwei
                21000,           //
                1800.0,          //
                7.56,            // expected fee
            ),
            (
                1_000_000_000, // 1 gwei
                21000,         //
                2000.0,        //
                0.042,         // expected fee
            ),
        ];

        for (gas_price, gas_used, eth_usdt, expected) in test_cases {
            let fee = calculate_tx_fee_usdt(gas_price, gas_used, eth_usdt);
            assert!(
                (fee - expected).abs() < 0.001,
                "Failed for gas_price={}, gas_used={}, eth_price={}. Expected {}, got {}",
                gas_price,
                gas_used,
                eth_usdt,
                expected,
                fee
            );
        }
    }
}
