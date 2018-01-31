CREATE TABLE valid_share (
  id SERIAL PRIMARY KEY,
  created TIMESTAMP NOT NULL DEFAULT NOW(),
  address VARCHAR(100) NOT NULL,
  miner_alias VARCHAR(100) NOT NULL,
  shares BIGINT NOT NULL
  -- TODO enforce the character limit
);
CREATE TABLE found_block (
  block_id TEXT NOT NULL PRIMARY KEY,
  created TIMESTAMP NOT NULL DEFAULT NOW(),
  height BIGINT NOT NULL,
  status INTEGER NOT NULL
);
CREATE TABLE block_progress (
  id SERIAL PRIMARY KEY,
  created TIMESTAMP NOT NULL DEFAULT NOW(),
  block_depth BIGINT NOT NULL,
  block_id TEXT NOT NULL REFERENCES FOUND_BLOCK
);
CREATE TABLE miner_balance (
  id SERIAL PRIMARY KEY,
  created TIMESTAMP NOT NULL DEFAULT NOW(),
  address VARCHAR(100) NOT NULL,
  change BIGINT NOT NULL,
  payment_transaction TEXT,
  is_fee BOOLEAN NOT NULL
);
CREATE TABLE pool_payment (
  id SERIAL PRIMARY KEY,
  created TIMESTAMP NOT NULL DEFAULT NOW(),
  payment_transaction TEXT NOT NULL,
  fee BIGINT NOT NULL
);