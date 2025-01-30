use tx_fees::components::api::start_server;
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
    let db_pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await?;

    run_migrations(&db_pool).await?;
    tokio::select! {
        realtime_result = realtime(&db_pool, &rpc_url) => {
            realtime_result?;
        }
        server_result = start_server(db_pool.clone()) => {
            server_result?;
        }
        _ = tokio::signal::ctrl_c() => {
            println!("Shutting down...");
        }
    }

    Ok(())
}
