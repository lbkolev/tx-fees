use actix_web::{web, App, HttpResponse, HttpServer};
use regex::Regex;
use serde::Serialize;
use serde_json::json;
use tracing::{error, warn};
use utoipa::{OpenApi, ToSchema};
use utoipa_swagger_ui::SwaggerUi;

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
    let re = Regex::new(r"^0x[a-fA-F0-9]{64}$").unwrap();
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
            error!("Database error: {:?}", e);
            HttpResponse::InternalServerError().finish()
        }
    }
}

#[derive(OpenApi)]
#[openapi(
    paths(get_fee),
    components(schemas(TxFee)),
    tags(
        (name = "Transaction Fees", description = "Endpoint used to retrieve transaction fees")
    )
)]
struct ApiDoc;

pub async fn start_server(db_pool: sqlx::PgPool) -> std::io::Result<()> {
    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(db_pool.clone()))
            .service(
                SwaggerUi::new("/swagger-ui/{_:.*}")
                    .url("/api-docs/openapi.json", ApiDoc::openapi()),
            )
            .route("/fee/{tx_hash}", web::get().to(get_fee))
    })
    .bind("0.0.0.0:8080")?
    .run()
    .await
}
