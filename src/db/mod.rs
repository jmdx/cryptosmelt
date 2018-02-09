use blocktemplate::*;
use daemon_client::Transfer;
use miner::*;
use diesel::prelude::*;
use dotenv::dotenv;
use diesel;
use std::env;
use regex::Regex;
use r2d2_diesel::ConnectionManager;
use r2d2::Pool;
use db::schema::*;
use db::models::*;

mod schema;
pub mod models;

#[derive(Debug)]
pub struct BlockShare {
  pub shares: u64,
  pub address: String,
  pub is_fee: bool,
}

pub struct DbAccess {
  conn_pool: Pool<ConnectionManager<PgConnection>>,
}

impl DbAccess {
  pub fn new() -> DbAccess {
    dotenv().ok();
    let database_url = env::var("DATABASE_URL")
      .expect("DATABASE_URL must be set");
    let manager = ConnectionManager::<PgConnection>::new(database_url);
    let pool = Pool::builder().build(manager)
      .expect("Failed to create connection pool.");
    DbAccess {
      conn_pool: pool,
    }
  }

  pub fn is_connected(&self) -> bool {
    self.conn_pool.get().is_ok()
  }

  pub fn block_found(&self, block: SuccessfulBlock, miner: &Miner, job: &Job) {
    self.shares_accepted(miner, job);

    let new_block = NewFoundBlock {
      block_id: &block.id,
      height: job.height as i64,
      status: BlockStatus::Submitted.into(),
    };
    if let Ok(conn) = self.conn_pool.get() {
      let result = diesel::insert_into(found_block::table)
        .values(&new_block)
        .get_result::<FoundBlock>(&*conn);
      if let Err(err) = result {
        warn!("Block found, but could not be saved to database, block: {:?}, error: {:?}", block, err);
      }
    }
    else {
      warn!("No available database connection.")
    }
  }

  pub fn shares_accepted(&self, miner: &Miner, job: &Job) {
    let alias = match &miner.alias {
      &Some(ref a) => a.to_owned(),
      &None => "anonymous".to_owned(),
    };
    let new_shares = NewShare {
      address: &miner.address,
      miner_alias: &alias,
      shares: job.difficulty as i64,
    };

    if let Ok(conn) = self.conn_pool.get() {
      let result = diesel::insert_into(valid_share::table)
        .values(&new_shares)
        .get_result::<ValidShare>(&*conn);
      if let Err(err) = result {
        warn!("Failed saving shares, error: {:?}", err);
      }
    }
    else {
      warn!("No available database connection.")
    }
  }

  pub fn block_status(&self, block_id: &String, new_status: BlockStatus) {
    use db::schema::found_block::dsl;

    if let Ok(conn) = self.conn_pool.get() {
      let status_code: i32 = new_status.into();
      let result = diesel::update(dsl::found_block.find(block_id))
        .set(dsl::status.eq(status_code))
        .get_result::<FoundBlock>(&*conn);
      if let Err(err) = result {
        warn!("Failed saving block status, error: {:?}", err);
      }
    }
    else {
      warn!("No available database connection.")
    }
  }

  pub fn block_progress(&self, block_id: &String, progress: u64) {
    let new_progress = NewBlockProgress {
      block_depth: progress as i64,
      block_id,
    };

    if let Ok(conn) = self.conn_pool.get() {
      let result = diesel::insert_into(block_progress::table)
        .values(&new_progress)
        .get_result::<BlockProgress>(&*conn);
      if let Err(err) = result {
        warn!("Failed saving block progress, error: {:?}", err);
      }
    }
    else {
      warn!("No available database connection.")
    }
  }

  pub fn log_transfers(&self, transfers: &Vec<Transfer>, tx_hash: &String, fee: u64) {
    let balance_changes: Vec<_> = transfers.iter().map(|change| {
      NewMinerBalance {
        address: &change.address,
        change: -1 * change.amount as i64,
        payment_transaction: Some(&tx_hash),
        is_fee: false,
      }
    }).collect();

    let new_payment = NewPoolPayment {
      payment_transaction: &tx_hash,
      fee: fee as i64,
    };

    if let Ok(conn) = self.conn_pool.get() {
      let result = diesel::insert_into(miner_balance::table)
        .values(&balance_changes)
        .get_result::<MinerBalance>(&*conn);
      if let Err(err) = result {
        panic!("Payments initiated at {}, but failed to subtract payments from miner balances: {:?}",
               tx_hash, err);
      }

      let payment_result = diesel::insert_into(pool_payment::table)
        .values(&new_payment)
        .get_result::<PoolPayment>(&*conn);
      if let Err(err) = payment_result {
        warn!("Failed saving pool payment, error: {:?}", err);
      }
    }
    else {
      warn!("No available database connection.")
    }
  }

  pub fn distribute_balances(&self, reward: u64, block_id: &str, share_counts: Vec<BlockShare>, total_shares: u64) {
    self.block_status(&block_id.to_owned(), BlockStatus::Unlocked);
    let miner_balances: Vec<_> = share_counts.iter().map(
      |&BlockShare { ref shares, ref address, ref is_fee }| {
      let balance_change = (*shares as u128 * reward as u128) / total_shares as u128;
      NewMinerBalance {
        address: &address,
        change: balance_change as i64,
        payment_transaction: None,
        is_fee: *is_fee,
      }
    }).collect();
    if let Ok(conn) = self.conn_pool.get() {
      let result = diesel::insert_into(miner_balance::table)
        .values(&miner_balances)
        .get_result::<MinerBalance>(&*conn);
      if let Err(err) = result {
        warn!("Failed recording miner balances, error: {:?}, shares {:?}", err, share_counts);
      }
    }
    else {
      warn!("No available database connection.")
    }
  }

  pub fn get_hashrates(&self) -> Vec<MinerStats> {
    if let Ok(conn) = self.conn_pool.get() {
      let result = diesel::sql_query(
        "SELECT CAST(SUM(shares) AS BIGINT) AS shares, miner_alias, \
         date_trunc('hour', created) + date_part('minute', created)::int / 5 * interval '5 min' \
         AS created_minute \
         FROM valid_share WHERE created > now() - interval '24 hours' \
         GROUP BY miner_alias, created_minute \
         ORDER BY created_minute"
      ).load(&*conn);
      match result {
        Ok(stats) => stats,
        Err(err) => {
          warn!("Failed to get miner stats: {:?}", err);
          vec![]
        },
      }
    }
    else {
      vec![]
    }
  }

  pub fn hashrates_by_address(&self, address_pattern: &Regex, address: &str) -> Vec<MinerStats> {
    if !address_pattern.is_match(address) {
      // Checking against the address pattern is important - we're not using diesel's query builder
      // in this method, so we rely on the fact that addresses are alphanumeric to prevent SQL
      // injection.
      return vec![];
    }
    if let Ok(conn) = self.conn_pool.get() {
      let query = format!(
        "SELECT CAST(SUM(shares) AS BIGINT) AS shares, miner_alias, \
         date_trunc('hour', created) + date_part('minute', created)::int / 5 * interval '5 min' \
         AS created_minute \
         FROM valid_share WHERE address='{}' AND created > now() - interval '24 hours' \
         GROUP BY miner_alias, created_minute \
         ORDER BY created_minute",
        address,
      );
      let result = diesel::sql_query(query).load(&*conn);
      match result {
        Ok(stats) => stats,
        Err(err) => {
          warn!("Failed to get miner stats: {:?}", err);
          vec![]
        },
      }
    }
    else {
      vec![]
    }
  }

  pub fn transactions_by_address(&self, address: &str) -> Vec<MinerBalance> {
    use db::schema::miner_balance::dsl;
    if let Ok(conn) = self.conn_pool.get() {
      let result = dsl::miner_balance.filter(dsl::address.eq(address))
        .load(&*conn);
      match result {
        Ok(blocks) => blocks,
        Err(err) => {
          warn!("Failed to get transactions: {:?}", err);
          vec![]
        },
      }
    }
    else {
      vec![]
    }
  }

  pub fn pending_submitted_blocks(&self) -> Vec<FoundBlock> {
    use db::schema::found_block::dsl;
    if let Ok(conn) = self.conn_pool.get() {
      let submitted: i32 = BlockStatus::Submitted.into();
      let result = dsl::found_block.filter(dsl::status.eq(submitted))
        .load(&*conn);
      match result {
        Ok(blocks) => blocks,
        Err(err) => {
          warn!("Failed to get submitted blocks: {:?}", err);
          vec![]
        },
      }
    }
    else {
      vec![]
    }
  }

  pub fn all_blocks(&self) -> Vec<FoundBlock> {
    use db::schema::found_block::dsl;
    if let Ok(conn) = self.conn_pool.get() {
      let result = dsl::found_block.load(&*conn);
      match result {
        Ok(blocks) => blocks,
        Err(err) => {
          warn!("Failed to get blocks: {:?}", err);
          vec![]
        },
      }
    }
    else {
      vec![]
    }
  }

  pub fn last_unlocked_block_time(&self) -> Option<::chrono::NaiveDateTime> {
    use db::schema::found_block::dsl;
    use diesel::dsl::min;
    if let Ok(conn) = self.conn_pool.get() {
      let submitted: i32 = BlockStatus::Submitted.into();
      let result = dsl::found_block.select(min(dsl::created))
        .filter(dsl::status.eq(submitted))
        .load(&*conn);
      match result {
        Ok(time) => if time.len() > 0 { time[0] } else { None },
        Err(err) => {
          warn!("Failed to get submitted blocks: {:?}", err);
          None
        },
      }
    }
    else {
      None
    }
  }

  pub fn unpaid_shares(&self) -> Vec<ShareTotal> {
    let shares_begin_time = self.last_unlocked_block_time();
    if let Ok(conn) = self.conn_pool.get() {
      let where_clause = match shares_begin_time {
        Some(begin_time) => format!("WHERE created > '{}'", begin_time),
        None => "".to_owned(),
      };
      let query = format!(
        "SELECT address, CAST(SUM(shares) AS BIGINT) AS shares FROM valid_share {} GROUP BY address",
        where_clause,
      );
      let result = diesel::sql_query(query).load(&*conn);
      match result {
        Ok(shares) => shares,
        Err(err) => {
          warn!("Failed to get submitted blocks: {:?}", err);
          vec![]
        },
      }
    }
    else {
      vec![]
    }
  }

  pub fn miner_balance_totals(&self) -> Vec<MinerBalanceTotal> {
    if let Ok(conn) = self.conn_pool.get() {
      let result = diesel::sql_query(
        "SELECT CAST(SUM(change) AS BIGINT) AS amount, address FROM miner_balance GROUP BY address"
      ).load(&*conn);
      match result {
        Ok(shares) => shares,
        Err(err) => {
          warn!("Failed to get submitted blocks: {:?}", err);
          vec![]
        },
      }
    }
    else {
      vec![]
    }
  }
}
