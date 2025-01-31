CREATE TABLE IF NOT EXISTS batch_jobs (
    id BIGSERIAL PRIMARY KEY,
    start_time BIGINT NOT NULL,
    end_time BIGINT NOT NULL,
    start_block BIGINT,
    end_block BIGINT,
    status TEXT NOT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT NOW (),
    updated_at TIMESTAMP NOT NULL DEFAULT NOW ()
);
