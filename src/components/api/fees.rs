use actix_web::{web, HttpResponse};
use regex::Regex;
use serde::Serialize;
use serde_json::json;
use tracing::{error, info};
use utoipa::ToSchema;

#[derive(Serialize, ToSchema)]
#[schema(example = json!({
    "tx_hash": "0x05f23901ca4a9f69e3ff0af3dec39f2876000974fc9d64f53897bf5ac5e3e700",
    "block_hash": "0x...",
    "block_number": 12345,
    "fee_usdt": 1.23,
    "eth_usdt_ratio": 1800.0
}))]
pub struct TxFee {
    tx_hash: String,
    block_hash: String,
    block_number: i64,
    fee_usdt: f64,
    eth_usdt_ratio: f64,
}

// used to sanity check the user tx_hash input
fn is_valid_tx_hash(tx_hash: &str) -> bool {
    let re = Regex::new(r"^0x([A-Fa-f0-9]{64})$").unwrap();
    re.is_match(tx_hash)
}

#[utoipa::path(
    get,
    path = "/v1/fees/{tx_hash}",
    params(
        ("tx_hash" = String, Path, description = "Ethereum transaction hash")
    ),
    responses(
        (status = 200, description = "Transaction fee details", body = TxFee),
        (status = 400, description = "Invalid transaction hash format"),
        (status = 404, description = "Transaction not found"),
        (status = 500, description = "Internal server error"),
    )
)]
pub async fn get_fee(db_pool: web::Data<sqlx::PgPool>, tx_hash: web::Path<String>) -> HttpResponse {
    let tx_hash_str = tx_hash.into_inner();

    // validate tx_hash format
    if !is_valid_tx_hash(&tx_hash_str) {
        error!(
            tx_hash = %tx_hash_str,
            "Invalid transaction hash format received"
        );
        return HttpResponse::BadRequest()
            .json(json!({"error": "Invalid transaction hash format"}));
    }

    let row = sqlx::query!(
        "SELECT t.hash as tx_hash, t.block_hash, b.number as block_number,
                t.fee_usdt, b.eth_usdt as eth_usdt_ratio
         FROM txs t
         JOIN blocks b ON t.block_hash = b.hash
         WHERE t.hash = $1",
        tx_hash_str
    )
    .fetch_optional(db_pool.get_ref())
    .await;

    match row {
        Ok(Some(r)) => {
            info!(
                tx_hash = %tx_hash_str,
                block_number = r.block_number,
                "Transaction fee retrieved successfully"
            );
            HttpResponse::Ok().json(TxFee {
                tx_hash: r.tx_hash,
                block_hash: r.block_hash,
                block_number: r.block_number,
                fee_usdt: r.fee_usdt,
                eth_usdt_ratio: r.eth_usdt_ratio,
            })
        }
        Ok(None) => HttpResponse::NotFound().finish(),
        Err(e) => {
            error!(
                tx_hash = %tx_hash_str,
                error = ?e,
                "Database error occurred while fetching transaction"
            );
            HttpResponse::InternalServerError().finish()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_valid_tx_hash() {
        let cases = vec![
            (
                "0x05f23901ca4a9f69e3ff0af3dec39f2876000974fc9d64f53897bf5ac5e3e700",
                true,
            ), // valid hash
            (
                "0x05F23901CA4A9F69E3FF0AF3DEC39F2876000974FC9D64F53897BF5AC5E3E700",
                true,
            ), // valid hash uppercase
            ("0x12345", false), // too short
            (
                "0x05f23901ca4a9f69e3ff0af3decQQ9f2876000974fc9d64f53897bf5ac5e3e700",
                false,
            ), // invalid characters
            (
                "abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890ab",
                false,
            ), // Missing 0x prefix
        ];

        for (tx_hash, expected) in cases {
            assert_eq!(
                is_valid_tx_hash(tx_hash),
                expected,
                "Failed for {}",
                tx_hash
            );
        }
    }
}
