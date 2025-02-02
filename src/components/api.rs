pub mod fees;
pub mod jobs;

use actix_web::{dev::Server, web, App, HttpServer};
use sqlx::PgPool;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use std::net::TcpListener;

use crate::components::api::fees::__path_get_fee;
use crate::components::api::fees::{get_fee, TxFee};
use crate::components::api::jobs::__path_create_batch_job;
use crate::components::api::jobs::{create_batch_job, BatchJobRequest, BatchJobResponse};

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
            .service(
                web::scope("/v1")
                    .route("/fees/{tx_hash}", web::get().to(get_fee))
                    .route("/jobs", web::post().to(create_batch_job)),
            )
    })
    .listen(listener)?
    .run();

    Ok(server)
}
