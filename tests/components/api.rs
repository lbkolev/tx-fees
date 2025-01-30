use crate::utils::{setup_test_db, teardown_test_db};
use tx_fees::components::api::start_server;

#[tokio::test]
async fn ww() {
    let (pool, db_name) = setup_test_db().await.unwrap();
    start_server(pool.clone()).await.unwrap();

    teardown_test_db(pool, &db_name).await.unwrap();
}
