use std::str::FromStr;

use alloy::{
    primitives::Address,
    providers::{ProviderBuilder, RootProvider, WsConnect},
    pubsub::PubSubFrontend,
};
use sqlx::PgPool;

#[derive(Debug)]
pub struct FeeTrackerConfig {
    // connection configs
    pub db_pool: PgPool,
    pub provider: RootProvider<PubSubFrontend>,

    pub pool_address: Address,
    pub price_pair: String,
    // memory management
    //pub max_seen_txs: usize,
    //pub max_seen_blocks: usize,
    //pub cleanup_interval: usize,
}

impl FeeTrackerConfig {
    pub async fn new(
        db_pool: PgPool,
        rpc_url: String,
        pool_address: String,
        price_pair: String,
    ) -> Self {
        let provider = ProviderBuilder::new()
            .on_ws(WsConnect::new(rpc_url))
            .await
            .expect("Unable to initialise WS Provider");

        Self {
            db_pool,
            provider,
            pool_address: Address::from_str(&pool_address).expect("Invalid pool address"),
            price_pair,
        }
    }
}

//
pub struct JobExecutorConfig {
    pub db_pool: PgPool,
    pub provider: RootProvider<PubSubFrontend>,
    pub redis_client: redis::Client,

    pub pool_address: Address,
    pub price_pair: String,
}

impl JobExecutorConfig {
    pub async fn new(
        db_pool: PgPool,
        rpc_url: String,
        redis_url: String,
        pool_address: String,
        price_pair: String,
    ) -> Self {
        let provider = ProviderBuilder::new()
            .on_ws(WsConnect::new(rpc_url))
            .await
            .expect("Unable to initialise WS Provider");

        let redis_client = redis::Client::open(redis_url).expect("Failed to create Redis client");

        Self {
            db_pool,
            provider,
            redis_client,
            pool_address: Address::from_str(&pool_address).expect("Invalid pool address"),
            price_pair,
        }
    }
}

#[derive(Debug)]
pub struct ServerConfig {
    pub db_pool: PgPool,
    pub redis_client: redis::Client,

    /// The host to bind the API to
    pub host: String,
    /// The port to bind the API to
    pub port: u16,
}

impl ServerConfig {
    pub fn new(db_pool: PgPool, redis_url: String, host: String, port: u16) -> Self {
        let redis_client = redis::Client::open(redis_url).expect("Failed to create Redis client");

        Self {
            db_pool,
            redis_client,
            host,
            port,
        }
    }
}
