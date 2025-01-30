use sqlx::{postgres::PgPoolOptions, Pool, Postgres};
use uuid::Uuid;

pub async fn setup_test_db() -> std::result::Result<(Pool<Postgres>, String), sqlx::Error> {
    let db_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgresql://user:password@0.0.0.0:5432/".to_string());
    let db_pool = PgPoolOptions::new()
        .connect(&format!("{}postgres", db_url))
        .await?;

    let db_name = format!("test_{}", Uuid::new_v4().simple());
    let db_url = format!("{}{}", db_url, db_name);

    sqlx::query(&format!("CREATE DATABASE {}", db_name))
        .execute(&db_pool)
        .await?;

    let pool = PgPoolOptions::new().connect(&db_url).await?;
    sqlx::migrate!("./migrations").run(&pool).await?;

    Ok((pool, db_name))
}

pub async fn teardown_test_db(
    pool: Pool<Postgres>,
    db_name: &String,
) -> std::result::Result<(), sqlx::Error> {
    // disconnect all connections from the pool or we won't be able to teardown
    drop(pool);

    let db_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgresql://user:password@0.0.0.0:5432/postgres".to_string());
    let master_pool = PgPoolOptions::new().connect(&db_url).await?;

    sqlx::query(&format!("DROP DATABASE IF EXISTS {}", db_name))
        .execute(&master_pool)
        .await?;

    Ok(())
}
