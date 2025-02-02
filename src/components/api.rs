pub mod fees;
pub mod jobs;

use std::net::TcpListener;

use actix_web::{dev::Server, web, App, HttpResponse, HttpServer};
use sqlx::PgPool;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use crate::{
    components::api::{
        fees::{__path_get_tx_fee, get_tx_fee, TxFee},
        jobs::{
            __path_create_batch_job, __path_get_job_status, create_batch_job, get_job_status,
            BatchJobRequest, BatchJobResponse,
        },
    },
    configs::ServerConfig,
};

#[derive(OpenApi)]
#[openapi(
    paths(get_tx_fee, create_batch_job, get_job_status,),
    components(schemas(TxFee, BatchJobRequest, BatchJobResponse))
)]
struct ApiDoc;

pub struct ServerApp {
    port: u16,
    server: Server,
}

impl ServerApp {
    pub async fn build(config: ServerConfig) -> eyre::Result<Self> {
        let listener = TcpListener::bind(format!("{}:{}", config.host, config.port))?;
        let port = listener.local_addr().unwrap().port();
        let server = start_server(listener, config.db_pool, config.redis_client)?;

        Ok(Self { port, server })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub async fn run_until_stopped(self) -> eyre::Result<()> {
        self.server
            .await
            .map_err(|e| eyre::eyre!("Server crashed: failed to accept new connections - {}", e))
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
            // used to check the healthiness of the Server,
            // for example by load balancers
            .route(
                "/health",
                web::get().to(|| async { HttpResponse::Ok().finish() }),
            )
            .service(
                SwaggerUi::new("/swagger-ui/{_:.*}")
                    .url("/api-docs/openapi.json", ApiDoc::openapi()),
            )
            .service(
                web::scope("/v1")
                    .route("/fees/{tx_hash}", web::get().to(get_tx_fee))
                    .route("/jobs", web::post().to(create_batch_job))
                    .route("/jobs/{job_id}", web::get().to(get_job_status)),
            )
    })
    .listen(listener)?
    .run();

    Ok(server)
}
