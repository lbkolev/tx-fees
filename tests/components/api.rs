use crate::utils::{spawn_app, teardown_test_db};
use reqwest::Client;
use serial_test::serial;
use sqlx::PgPool;

async fn insert_mock_data(db_pool: &PgPool) {
    sqlx::query!(
        "INSERT INTO blocks (hash, number, eth_usdt) VALUES ($1, $2, $3)",
        "0x7D0AA91b12d31755D2fc99d22e09947936E00474",
        123456,
        3500.0
    )
    .execute(db_pool)
    .await
    .expect("Failed to insert mock block");

    sqlx::query!(
        "INSERT INTO txs (hash, block_hash, fee_usdt) VALUES ($1, $2, $3)",
        "0xc0dc5948835b50337e8548dc7518dafd3f65b12b1e5f381b7f16684124924a54",
        "0x7D0AA91b12d31755D2fc99d22e09947936E00474",
        15.0
    )
    .execute(db_pool)
    .await
    .expect("Failed to insert mock transaction");
}

#[tokio::test]
async fn test_get_fee_success() {
    let app = spawn_app().await;
    let client = Client::new();
    insert_mock_data(&app.db_pool).await;

    let tx_hash = "0xc0dc5948835b50337e8548dc7518dafd3f65b12b1e5f381b7f16684124924a54";
    let response = client
        .get(&format!("{}/fee/{tx_hash}", &app.address))
        .send()
        .await
        .expect("Failed to execute request.");

    assert_eq!(response.status(), 200);
    let body: serde_json::Value = response.json().await.expect("Failed to parse JSON");
    assert_eq!(body["tx_hash"], tx_hash);
    assert_eq!(
        body["block_hash"],
        "0x7D0AA91b12d31755D2fc99d22e09947936E00474"
    );
    assert_eq!(body["block_number"], 123456);
    assert_eq!(body["fee_usdt"], 15.0);
    assert_eq!(body["eth_usdt_ratio"], 3500.0);

    teardown_test_db(app).await.unwrap();
}

#[tokio::test]
async fn test_get_fee_invalid_tx_hash() {
    let app = spawn_app().await;
    let client = Client::new();

    let response = client
        .get(&format!("{}/fee/invalidhash", &app.address))
        .send()
        .await
        .expect("Failed to execute request.");

    assert_eq!(response.status(), 400);
    let body: serde_json::Value = response.json().await.expect("Failed to parse JSON");
    assert_eq!(body["error"], "Invalid transaction hash format");

    teardown_test_db(app).await.unwrap();
}

#[tokio::test]
async fn test_get_fee_tx_not_found() {
    let app = spawn_app().await;
    let client = Client::new();

    let tx_hash = "0xc0dc5948835b50337e8548dc7518dafd3f65b12b1e5f381b7f16684124924a54";
    let response = client
        .get(&format!("{}/fee/{}", &app.address, tx_hash))
        .send()
        .await
        .expect("Failed to execute request.");

    assert_eq!(response.status(), 404);
    teardown_test_db(app).await.unwrap();
}
