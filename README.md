# tx-fees
Combination of components for real-time & historical transaction fee calculation.Each component can be ran independently from one another,
which allows for horizontal scaling based on the load each component is handling (`FeeTracker` is an exception since it doesn't make sense to have
multiple instances of it running at the same time).

The DB Schema configuration can be found at `migrations/`. We're applying the schema on the fly when the application starts
so there's no need to run the migrations manually.

## Components
- Real-time tx fee tracker
- Historical tx fee job executor
- REST API exposing the actions and data of the above components

### Real-time Tx fee tracker
- Tracks the tx fees in USDT for the provided liquidity pool (in our case UniswapV3's `ETH/USDC` pool)
- The tx fees are calculated in real-time based on the latest ETH/USDT price at each block commit
- The tx fees are stored in a DB for later retrieval by the REST API.

### Historical Tx fee job executor
- Executes batch jobs for historical data processing
- The job executor is responsible for fetching the historical data from the blockchain and calculating the tx fees in USDT for a given time range
- It's designed to be horizontally scalable. It can be run on multiple instances and each instance will pick up a different job from the queue.

### REST API
Exposes the actions and data of the above components (`FeeTracker` and `JobExecutor`).
Additional API documentation can be found at `[http://localhost:8080/swagger-ui/](http://localhost:8080/swagger-ui)`.
- Endpoints:
  - `GET /v1/tx-fees/{tx_hash}` - returns the real-time tx fees in USDT for the provided liquidity pool
  - `POST /v1/jobs` - creates a new batch job for historical data
  - `GET /v1/jobs/{job_id}` - returns the status of the job with the provided id


# Setup
The whole setup is dockerised — to run the application & all the miscellaneous services with default configuration (should be sufficient):
```bash
docker-compose up -d --build
```
to run with custom configuration setup the environment variables in the `.envs` file.

Alternatively, you can run the application without docker:
```bash
docker-compose up -d redis postgres
cargo r -- --components fee-tracker,job-executor,api
```

To run the tests locally:
```bash
docker-compose up -d redis postgres
cargo test
```

# Optimisations

- Each component can be ran independently from one another. This way we can scale each component separately based on the
load it's handling.

- The only two external providers we make use of are - ethereum WS RPC provider and the ETH/USDT price provider (Binance).
 Not going through

- Since we're interested in the tx fees value only once a tx is committed (i.e a block is created), we're
getting the ETH/USDT price only once per block (considering the block contains TXs that interact with our pool), so
`FeeTracker` is never getting rate limited by our eth/usdt price provider.

- The `JobExecutor` is designed to be horizontally scalable. It can be run on multiple instances and each instance will
pick up a different job from the queue. This way we can process multiple jobs in parallel. Esp. helpful
since the historical data processing can be quite intensive and time-consuming with large enough `start_time` and `end_time` timestamp ranges.

- The `Api` is also designed to be horizontally scalable. It can be run on multiple instances and each instance will
spin up a fresh new API server. This way we can handle more requests in parallel.
