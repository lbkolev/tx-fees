use actix_web::{dev::Server, web, App, HttpResponse, HttpServer};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::PgPool;
use tracing::{error, warn};
use utoipa::{OpenApi, ToSchema};
use utoipa_swagger_ui::SwaggerUi;

use std::{
    fmt::Display,
    net::TcpListener,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Serialize, ToSchema)]
struct TxFee {
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
    path = "/fee/{tx_hash}",
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
async fn get_fee(db_pool: web::Data<sqlx::PgPool>, tx_hash: web::Path<String>) -> HttpResponse {
    let tx_hash_str = tx_hash.into_inner();

    // validate tx_hash format
    if !is_valid_tx_hash(&tx_hash_str) {
        warn!("Invalid transaction hash received: {}", tx_hash_str);
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
        Ok(Some(r)) => HttpResponse::Ok().json(TxFee {
            tx_hash: r.tx_hash,
            block_hash: r.block_hash,
            block_number: r.block_number,
            fee_usdt: r.fee_usdt,
            eth_usdt_ratio: r.eth_usdt_ratio,
        }),
        Ok(None) => HttpResponse::NotFound().finish(),
        Err(e) => {
            error!("db err: {:?}", e);
            HttpResponse::InternalServerError().finish()
        }
    }
}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct BatchJobRequest {
    start_time: i64,
    end_time: i64,
}

#[derive(Serialize, ToSchema)]
pub struct BatchJobResponse {
    job_id: i64,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum BatchJobStatus {
    Pending,
    InProgress,
    Completed,
}

impl Display for BatchJobStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BatchJobStatus::Pending => write!(f, "pending"),
            BatchJobStatus::InProgress => write!(f, "in_progress"),
            BatchJobStatus::Completed => write!(f, "completed"),
        }
    }
}

// used by the batch_job endpoint
// verifies:
//  1. start time is after defi started
//  2. end time is not in the future
//  3. start time is lower than end time
fn is_valid_time_range(start_time: i64, end_time: i64) -> bool {
    const DEFI_START: i64 = 1514764800; // 2018-01-01
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    start_time >= DEFI_START && end_time <= now && start_time < end_time
}

#[utoipa::path(
    post,
    path = "/batch_job",
    request_body = BatchJobRequest,
    responses(
        (status = 200, description = "Batch job created", body = BatchJobResponse),
        (status = 400, description = "Invalid time range"),
        (status = 500, description = "Internal server error"),
    )
)]
async fn create_batch_job(
    db_pool: web::Data<PgPool>,
    redis: web::Data<redis::Client>,
    req: web::Json<BatchJobRequest>,
) -> HttpResponse {
    if !is_valid_time_range(req.start_time, req.end_time) {
        return HttpResponse::BadRequest()
            .json(json!({"error": "Invalid time range. Must be between 2018-01-01 and now"}));
    }

    let job_id = match sqlx::query!(
        "INSERT INTO batch_jobs (start_time, end_time, status) VALUES ($1, $2, $3) RETURNING id",
        req.start_time,
        req.end_time,
        BatchJobStatus::Pending.to_string()
    )
    .fetch_one(db_pool.get_ref())
    .await
    {
        Ok(row) => row.id,
        Err(e) => {
            error!("db err: {:?}", e);
            return HttpResponse::InternalServerError().finish();
        }
    };

    let mut conn = match redis.get_ref().get_multiplexed_async_connection().await {
        Ok(conn) => conn,
        Err(e) => {
            error!("redis err: {:?}", e);
            return HttpResponse::InternalServerError().finish();
        }
    };

    if let Err(e) = redis::cmd("RPUSH")
        .arg("batch_jobs")
        .arg(job_id)
        .query_async::<()>(&mut conn)
        .await
    {
        error!("redis err: {:?}", e);
        return HttpResponse::InternalServerError().finish();
    }

    HttpResponse::Ok().json(BatchJobResponse { job_id })
}

#[derive(OpenApi)]
#[openapi(
    paths(
        get_fee,
        create_batch_job
    ),
    components(
        schemas(
            TxFee,
            BatchJobRequest,
            BatchJobResponse
        )
    ),
    tags(
        (name = "Transaction Fees", description = "Endpoint used to retrieve transaction fees"),
        (name = "Batch Jobs", description = "Endpoint for managing fee calculation batch jobs")
    )
)]
struct ApiDoc;

pub struct AppServer {
    port: u16,
    server: Server,
}

impl AppServer {
    pub async fn build(
        host: String,
        port: u16,
        db_pool: PgPool,
        redis_client: redis::Client,
    ) -> eyre::Result<Self> {
        let listener = TcpListener::bind(format!("{}:{}", host, port))?;
        let port = listener.local_addr().unwrap().port();
        let server = start_server(listener, db_pool, redis_client)?;

        Ok(Self { port, server })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub async fn run_until_stopped(self) -> std::result::Result<(), std::io::Error> {
        self.server.await
    }
}

fn start_server(
    listener: TcpListener,
    db_pool: PgPool,
    redis_client: redis::Client,
) -> std::result::Result<Server, std::io::Error> {
    let server = HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(db_pool.clone()))
            .app_data(web::Data::new(redis_client.clone()))
            .service(
                SwaggerUi::new("/swagger-ui/{_:.*}")
                    .url("/api-docs/openapi.json", ApiDoc::openapi()),
            )
            .route("/fee/{tx_hash}", web::get().to(get_fee))
            .route("/batch_job", web::post().to(create_batch_job))
    })
    .listen(listener)?
    .run();

    Ok(server)
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

    #[test]
    fn test_is_valid_time_range() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        let cases = vec![
            (1514764800, now, true),         // DeFi start to now - valid
            (1514764799, now, false),        // before DeFi start - invalid
            (1514764800, 1514764801, true),  // minimal valid range
            (now + 1, now + 2, false),       // future range - invalid
            (1514764800, 1514764800, false), // same timestamps - invalid
            (1514764801, 1514764800, false), // end before start - invalid
        ];

        for (start, end, expected) in cases {
            assert_eq!(
                is_valid_time_range(start, end),
                expected,
                "Failed for start: {}, end: {}",
                start,
                end
            );
        }
    }
}
