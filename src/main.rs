use clap::Parser;
use eyre::Result;
use secrecy::ExposeSecret;
use sqlx::{postgres::PgPoolOptions, PgPool};

use tracing::info;
use tx_fees::{
    args::{Args, Component},
    components::{api::ServerApp, fee_tracker::FeeTrackerApp, job_executor::JobExecutorApp},
    configs::{FeeTrackerConfig, JobExecutorConfig, ServerConfig},
};

async fn run_migrations(db_pool: &PgPool) -> Result<()> {
    sqlx::migrate!("./migrations").run(db_pool).await?;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt().with_thread_ids(true).init();
    let args = Args::parse();
    info!(args=?args);

    /*
     setup all the external provider connections (db, /websocket/ RPC, redis etc.)
    */
    let db_pool = PgPoolOptions::new()
        .connect(args.database_url.expose_secret())
        .await?;

    // run the migrations on startup to ensure the db schema is up to date
    // and for the sake of simplicity
    run_migrations(&db_pool).await?;

    let mut tasks = vec![];
    if args.components.contains(&Component::FeeTracker) {
        tasks.push(tokio::spawn(FeeTrackerApp::run(
            FeeTrackerConfig::new(
                db_pool.clone(),
                args.rpc_url.expose_secret().to_string().clone(),
                args.liquidity_pool.clone(),
                args.price_pair.clone(),
            )
            .await,
        )));
    }

    if args.components.contains(&Component::JobExecutor) {
        tasks.push(tokio::spawn(JobExecutorApp::run(
            JobExecutorConfig::new(
                db_pool.clone(),
                args.rpc_url.expose_secret().to_string().clone(),
                args.redis_url.expose_secret().to_string().clone(),
                args.liquidity_pool.clone(),
                args.price_pair.clone(),
            )
            .await,
        )));
    }

    if args.components.contains(&Component::Api) {
        tasks.push(tokio::spawn(
            ServerApp::build(ServerConfig::new(
                db_pool.clone(),
                args.redis_url.expose_secret().to_string().clone(),
                args.api_host,
                args.api_port,
            ))
            .await?
            .run_until_stopped(),
        ));
    }

    let _ = futures::future::select_all(tasks).await;

    Ok(())
}
