CREATE TABLE blocks (
    hash TEXT PRIMARY KEY,
    number BIGINT NOT NULL,
    eth_usdt DOUBLE PRECISION NOT NULL,
    committed_at TIMESTAMP DEFAULT NOW ()
);

COMMENT ON COLUMN blocks.eth_usdt IS 'ETH/USDT ratio at block commit time. We are only interested in the ratio once the block gets committed, not in any of the ~12s it takes to assemble all the txs.';
