use std::sync::{Arc};
use config::*;
use blocktemplate::*;
use influx_db_client::{Client};
use daemon_client::Transfer;
use miner::*;
use influx_db_client::*;
use serde_json::Value as SjValue;

#[derive(Debug)]
pub struct BlockShare {
  pub shares: u64,
  pub address: String,
  pub is_fee: bool,
}

pub struct DbAccess {
  client: Client,
}

impl DbAccess {
  pub fn new(config: Arc<Config>) -> DbAccess {
    let client = Client::new(config.influx_url.to_owned(), "cryptosmelt".to_owned());
    DbAccess {
      client,
    }
  }

  pub fn is_connected(&self) -> bool {
    self.client.ping()
  }

  pub fn block_found(&self, block: SuccessfulBlock, miner: &Miner, job: &Job) {
    let mut share_log = Point::new("valid_share");
    share_log.add_tag("address", Value::String(miner.address.to_owned()));
    let alias = match &miner.alias {
      &Some(ref a) => a.to_owned(),
      &None => "anonymous".to_owned(),
    };
    share_log.add_tag("alias", Value::String(alias));
    share_log.add_field("value", Value::Integer(job.difficulty as i64));
    let mut submission_log = Point::new("block_status");
    submission_log.add_tag("block", Value::String(block.id.to_owned()));
    submission_log.add_field("height", Value::Integer(job.height as i64));
    submission_log.add_field("status", Value::String("submitted".to_owned()));
    let mut to_insert = Points::new(share_log);
    to_insert.push(submission_log);
    if let Err(err) = self.client.write_points(to_insert, Some(Precision::Seconds), None) {
      warn!("Block found, but could not be saved to database, block: {:?}, error: {:?}", block, err);
    }
  }

  pub fn shares_accepted(&self, miner: &Miner, job: &Job) {
    let mut share_log = Point::new("valid_share");
    share_log.add_tag("address", Value::String(miner.address.to_owned()));
    let alias = match &miner.alias {
      &Some(ref a) => a.to_owned(),
      &None => "anonymous".to_owned(),
    };
    share_log.add_tag("alias", Value::String(alias));
    share_log.add_field("value", Value::Integer(job.difficulty as i64));
    if let Err(err) = self.client.write_point(share_log, Some(Precision::Seconds), None) {
      warn!("Failed saving shares, error: {:?}", err);
    }
  }

  pub fn block_status(&self, block_id: &String, status: &str) {
    let mut orphaned = Point::new("block_status");
    orphaned.add_tag("block", Value::String(block_id.to_owned()));
    orphaned.add_field("status", Value::String(status.to_owned()));
    if let Err(err) = self.client.write_point(orphaned, Some(Precision::Seconds), None) {
      warn!("Failed saving block status, error: {:?}", err);
    }
  }

  pub fn block_progress(&self, block_id: &String, progress: u64) {
    let mut unlocked = Point::new("block_progress");
    unlocked.add_tag("block", Value::String(block_id.to_owned()));
    unlocked.add_field("depth", Value::Integer(progress as i64));
    if let Err(err) = self.client.write_point(unlocked, Some(Precision::Seconds), None) {
      warn!("Failed saving block progress, error: {:?}", err);
    }
  }

  pub fn log_transfers(&self, transfers: &Vec<Transfer>, tx_hash: &String, fee: u64) {
    let balance_changes: Vec<_> = transfers.iter().map(|change| {
      let mut balance_insert = Point::new("miner_balance");
      balance_insert.add_tag("address", Value::String(change.address.to_owned()));
      balance_insert.add_field("change", Value::Integer(-1 * change.amount as i64));
      balance_insert.add_tag("payment_transaction", Value::String(tx_hash.to_owned()));
      balance_insert
    }).collect();
    self.client.write_points(Points::create_new(balance_changes), Some(Precision::Seconds), None)
      // TODO after migrating to postgres, probably add some retrying logic here
      .expect(&format!("Payments initiated at {}, but failed to subtract payments from miner balances",
                      tx_hash));
    let mut payment_log = Point::new("pool_payment");
    payment_log.add_tag("transaction_hash", Value::String(tx_hash.to_owned()));
    payment_log.add_field("fee", Value::Integer(fee as i64));
    self.client.write_point(payment_log, Some(Precision::Seconds), None)
      .expect("Payments recorded, but failed to record pool payment transaction");
  }

  pub fn distribute_balances(&self, reward: u64, block_id: &str, share_counts: Vec<BlockShare>, total_shares: u64) {
    let mut share_inserts = Points::create_new(vec![]);
    for BlockShare { shares, address, is_fee } in share_counts {
      let balance_change = (shares as u128 * reward as u128) / total_shares as u128;
      let mut share_insert = Point::new("miner_balance");
      share_insert.add_tag("address", Value::String(address.to_owned()));
      share_insert.add_field("change", Value::Integer(balance_change as i64));
      share_insert.add_field("is_fee", Value::Boolean(is_fee));
      share_inserts.push(share_insert);
    }
    let mut unlocked = Point::new("block_status");
    unlocked.add_tag("block", Value::String(block_id.to_owned()));
    unlocked.add_field("status", Value::String("unlocked".to_owned()));
    if let Err(err) = self.client.write_point(unlocked, Some(Precision::Seconds), None) {
      warn!("Failed saving unlocked block, error: {:?}", err);
    }
    if let Err(err) = self.client.write_points(share_inserts, Some(Precision::Seconds), None) {
      warn!("Failed to distribute shares, error: {:?}", err);
    }
  }

  pub fn query(&self, query: &str) -> Vec<Vec<SjValue>> {
    let results = self.client.query(query, None);
    match results {
      Ok(Some(nodes)) => {
        nodes.iter().flat_map(|node| {
          node.series.iter().flat_map(|series| {
            series.iter().flat_map(|some_series| some_series.values.clone())
          })
        }).collect()
      },
      err => {
        warn!("Database error {:?}", err);
        vec![]
      },
    }
  }
}
