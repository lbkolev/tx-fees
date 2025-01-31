use tx_fees::components::api::AppServer;
use tx_fees::components::realtime_data::realtime;

use eyre::Result;
use sqlx::{postgres::PgPoolOptions, PgPool};

async fn run_migrations(db_pool: &PgPool) -> Result<()> {
    sqlx::migrate!("./migrations").run(db_pool).await?;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let rpc_url = std::env::var("ETH_WS_RPC_URL")
        .unwrap_or_else(|_| "wss://mainnet.gateway.tenderly.co/".to_string());
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgresql://user:password@0.0.0.0:5432/tx_fees".to_string());
    let api_host = std::env::var("API_HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
    let api_port = std::env::var("API_PORT")
        .unwrap_or_else(|_| "8080".to_string())
        .parse::<u16>()
        .expect("Invalid API_PORT");

    let db_pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await?;

    run_migrations(&db_pool).await?;
    tokio::select! {
        realtime_result = realtime(&db_pool, &rpc_url) => {
            realtime_result?;
        }
        server_result = AppServer::build(api_host, api_port, db_pool.clone()).await?.run_until_stopped() => {
            server_result?;
        }
    }

    Ok(())
}
