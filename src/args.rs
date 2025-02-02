use clap::{Parser, ValueEnum};
use secrecy::SecretString;

#[derive(Debug, Clone, Eq, PartialEq, ValueEnum)]
pub enum Component {
    FeeTracker,
    JobExecutor,
    Api,
}

#[derive(Parser, Debug)]
#[command(author, version, about)]
pub struct Args {
    #[arg(
        long,
        env = "ETH_WS_RPC_URL",
        default_value = "wss://mainnet.gateway.tenderly.co/"
    )]
    pub rpc_url: SecretString,

    #[arg(
        long,
        env = "DATABASE_URL",
        default_value = "postgresql://user:password@0.0.0.0:5432/tx_fees"
    )]
    pub database_url: SecretString,

    #[arg(long, env = "REDIS_URL", default_value = "redis://127.0.0.1:6379")]
    pub redis_url: SecretString,

    #[arg(long, env = "API_HOST", default_value = "0.0.0.0")]
    pub api_host: String,

    #[arg(long, env = "API_PORT", default_value = "8080")]
    pub api_port: u16,

    #[arg(
        long,
        value_enum,
        env = "COMPONENTS",
        value_delimiter = ',',
        default_value = "fee-tracker,job-executor,api"
    )]
    pub components: Vec<Component>,

    #[arg(
        long,
        env = "LIQUIDITY_POOL",
        default_value = "0x88e6a0c2ddd26feeb64f039a2c41296fcb3f5640" // UniswapV3's ETH-USDC
    )]
    pub liquidity_pool: String,

    #[arg(long, env = "PRICE_PAIR", default_value = "ETHUSDT")]
    pub price_pair: String,
}
