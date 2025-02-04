use reqwest::Client;
use serde_json::json;
use serial_test::serial;
use sqlx::PgPool;

use crate::utils::{spawn_test_server, teardown_test_db};

lazy_static::lazy_static! {
    static ref CLIENT: Client = Client::new();
}

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
#[serial]
async fn test_get_fee_success() {
    let app = spawn_test_server().await;
    insert_mock_data(&app.db_pool).await;

    let tx_hash = "0xc0dc5948835b50337e8548dc7518dafd3f65b12b1e5f381b7f16684124924a54";
    let response = CLIENT
        .get(format!("{}/v1/fees/{tx_hash}", &app.address))
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
    let app = spawn_test_server().await;

    let response = CLIENT
        .get(format!("{}/v1/fees/invalidhash", &app.address))
        .send()
        .await
        .expect("Failed to execute request.");

    assert_eq!(response.status(), 400);
    let body: serde_json::Value = response.json().await.expect("Failed to parse JSON");
    assert_eq!(body["error"], "Invalid transaction hash format");

    // Test with a valid hash but non-existent in the DB
    let tx_hash = "0xc0dc5948835b50337e8548dc7518dafd3f65b12b1e5f381b7f16684124924a54";
    let response = CLIENT
        .get(format!("{}/v1/fees/{}", &app.address, tx_hash))
        .send()
        .await
        .expect("Failed to execute request.");

    assert_eq!(response.status(), 404);

    teardown_test_db(app).await.unwrap();
}

#[tokio::test]
#[serial]
async fn test_job_success() {
    let app = spawn_test_server().await;

    let request = json!({
        "start_time": 1514764800,  // 2018-01-01
        "end_time": 1514851200     // 2018-01-02
    });

    let response = CLIENT
        .post(format!("{}/v1/jobs", &app.address))
        .json(&request)
        .send()
        .await
        .expect("Failed to execute request.");

    assert_eq!(response.status(), 201);

    // verify response contains job_id
    let body: serde_json::Value = response.json().await.expect("Failed to parse JSON");
    assert!(body["job_id"].as_i64().is_some());

    // verify there's a DB entry
    let job = sqlx::query!(
        "SELECT * FROM batch_jobs WHERE id = $1",
        body["job_id"].as_i64().unwrap()
    )
    .fetch_one(&app.db_pool)
    .await
    .expect("Failed to fetch job");

    assert_eq!(job.start_time, 1514764800);
    assert_eq!(job.end_time, 1514851200);
    assert_eq!(job.status, "pending");

    // verify the redis message queue entry is present and equal to the expected value
    let mut conn = app
        .redis_client
        .get_multiplexed_async_connection()
        .await
        .expect("Failed to connect to Redis");

    let job_id: Option<i64> = redis::cmd("RPOP")
        .arg("batch_jobs")
        .query_async(&mut conn)
        .await
        .expect("Failed to query Redis");

    assert_eq!(job_id, Some(body["job_id"].as_i64().unwrap()));

    teardown_test_db(app).await.unwrap();
}

#[tokio::test]
#[serial]
async fn test_job_invalid_time_range() {
    let app = spawn_test_server().await;

    let test_cases = vec![
        (
            json!({
                "start_time": 1514764799_i64,  // Before 2018
                "end_time": 1514851200_i64
            }),
            "time before DeFi start",
        ),
        (
            json!({
                "start_time": 1514851200_i64,
                "end_time": 1514764800_i64
            }),
            "end before start",
        ),
        (
            json!({
                "start_time": 1514764800_i64,
                "end_time": 1514764800_i64
            }),
            "same timestamps",
        ),
        (
            json!({
                "start_time": 32503680000_i64,
                "end_time": 32503766400_i64
            }),
            "future dates",
        ),
    ];

    for (request, test_case) in test_cases {
        let response = CLIENT
            .post(format!("{}/v1/jobs", &app.address))
            .json(&request)
            .send()
            .await
            .unwrap_or_else(|_| panic!("Failed to execute request for {}", test_case));

        assert_eq!(
            response.status(),
            400,
            "Expected 400 status for {}",
            test_case
        );
        let body: serde_json::Value = response.json().await.expect("Failed to parse JSON");
        assert_eq!(
            body["error"],
            "Invalid time range. Must be between 2018-01-01 and now"
        );
    }

    teardown_test_db(app).await.unwrap();
}

#[tokio::test]
#[serial]
async fn test_job_malformed_request() {
    let app = spawn_test_server().await;

    let test_cases = vec![
        (
            json!({
                "start_time": "not a number",
                "end_time": 1514851200_i64
            }),
            "invalid start_time type",
        ),
        (
            json!({
                "start_time": 1514764800_i64
                // missing end_time
            }),
            "missing end_time",
        ),
        (
            json!({
                // missing start_time
                "end_time": 1514851200_i64
            }),
            "missing start_time",
        ),
        (json!({}), "empty request"),
    ];

    for (request, test_case) in test_cases {
        let response = CLIENT
            .post(format!("{}/v1/jobs", &app.address))
            .json(&request)
            .send()
            .await
            .unwrap_or_else(|_| panic!("Failed to execute request for {}", test_case));

        assert_eq!(
            response.status(),
            400,
            "Expected 400 status for {}",
            test_case
        );
    }

    teardown_test_db(app).await.unwrap();
}

#[tokio::test]
#[serial]
async fn test_get_job_status() {
    let app = spawn_test_server().await;

    // First create a job
    let request = json!({
        "start_time": 1514764800,
        "end_time": 1514851200
    });

    let response = CLIENT
        .post(format!("{}/v1/jobs", &app.address))
        .json(&request)
        .send()
        .await
        .expect("Failed to create job");

    let job_id = response
        .json::<serde_json::Value>()
        .await
        .expect("Failed to parse JSON")["job_id"]
        .as_i64()
        .unwrap();

    // Then get its status
    let response = CLIENT
        .get(format!("{}/v1/jobs/{}", &app.address, job_id))
        .send()
        .await
        .expect("Failed to execute request");

    assert_eq!(response.status(), 200);

    let body: serde_json::Value = response.json().await.expect("Failed to parse JSON");
    assert_eq!(body["job_id"], job_id);
    assert_eq!(body["status"], "pending");
    assert_eq!(body["start_time"], 1514764800);
    assert_eq!(body["end_time"], 1514851200);

    // Test non-existent job
    let response = CLIENT
        .get(format!("{}/v1/jobs/99999", &app.address))
        .send()
        .await
        .expect("Failed to execute request");

    assert_eq!(response.status(), 404);

    teardown_test_db(app).await.unwrap();
}
