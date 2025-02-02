use sqlx::{postgres::PgPoolOptions, PgPool};
use uuid::Uuid;

use tx_fees::components::api::ServerApp;

pub async fn setup_test_db() -> std::result::Result<(PgPool, String), sqlx::Error> {
    let db_url = std::env::var("TEST_DATABASE_URL")
        .unwrap_or_else(|_| "postgresql://user:password@0.0.0.0:5432/".to_string());
    let db_pool = PgPoolOptions::new()
        .connect(&format!("{}postgres", db_url))
        .await?;

    let db_name = format!("test_{}", Uuid::new_v4().simple());
    let db_url = format!("{}{}", db_url, db_name);

    sqlx::query(&format!("CREATE DATABASE {}", db_name))
        .execute(&db_pool)
        .await?;

    let pool = PgPoolOptions::new()
        .max_connections(50)
        .connect(&db_url)
        .await?;
    sqlx::migrate!("./migrations").run(&pool).await?;

    Ok((pool, db_name))
}

pub async fn teardown_test_db(app: TestServer) -> std::result::Result<(), sqlx::Error> {
    // ensure there are no other connections active to the database
    drop(app.db_pool);

    let db_url = std::env::var("TEST_DATABASE_URL")
        .unwrap_or_else(|_| "postgresql://user:password@0.0.0.0:5432/postgres".to_string());

    let master_pool = PgPoolOptions::new().connect(&db_url).await?;

    // forcefully end any leftover connections
    sqlx::query(&format!(
        "SELECT pg_terminate_backend(pg_stat_activity.pid)
         FROM pg_stat_activity
         WHERE datname = '{}'
         AND pid <> pg_backend_pid();",
        app.db_name
    ))
    .execute(&master_pool)
    .await?;

    sqlx::query(&format!("DROP DATABASE IF EXISTS {}", app.db_name))
        .execute(&master_pool)
        .await?;

    Ok(())
}

#[derive(Debug)]
pub struct TestServer {
    pub address: String,
    pub port: u16,
    pub db_pool: PgPool,
    pub db_name: String,
    pub redis_client: redis::Client,
}

pub async fn spawn_test_server() -> TestServer {
    let (db_pool, db_name) = setup_test_db().await.unwrap();
    let redis_url =
        std::env::var("TEST_REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string());
    let redis_client = redis::Client::open(redis_url).expect("Failed to create Redis client");

    let server_app = ServerApp::build(
        "localhost".to_string(),
        0,
        db_pool.clone(),
        redis_client.clone(),
    )
    .await
    .expect("Failed to build the Server application.");

    let server_app_port = server_app.port();
    let _ = tokio::spawn(async move { server_app.run_until_stopped().await });

    TestServer {
        address: format!("http://localhost:{}", server_app_port),
        port: server_app_port,
        db_pool,
        db_name,
        redis_client,
    }
}
