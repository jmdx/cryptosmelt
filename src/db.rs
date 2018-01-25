use std::sync::{Arc};
use config::*;
use blocktemplate::*;
use influx_db_client::{Client};
use miner::*;
use influx_db_client::{Point, Points, Precision, Value as IxValue};

pub struct DbAccess {
  // TODO eventually this should be private
  pub client: Client,
}

impl DbAccess {
  pub fn new(config: Arc<Config>) -> DbAccess {
    let client = Client::new(config.influx_url.to_owned(), "cryptosmelt".to_owned());
    DbAccess {
      client,
    }
  }

  pub fn block_found(&self, block: SuccessfulBlock, miner: &Miner, job: &Job) {
    let mut share_log = Point::new("valid_share");
    share_log.add_tag("address", IxValue::String(miner.login.to_owned()));
    share_log.add_field("value", IxValue::Integer(job.difficulty as i64));
    let mut submission_log = Point::new("block_status");
    submission_log.add_tag("block", IxValue::String(block.id.to_owned()));
    submission_log.add_field("height", IxValue::Integer(job.height as i64));
    submission_log.add_field("status", IxValue::String("submitted".to_owned()));
    let mut to_insert = Points::new(share_log);
    to_insert.push(submission_log);
    if let Err(err) = self.client.write_points(to_insert, Some(Precision::Seconds), None) {
      warn!("Block found, but could not be saved to database, block: {:?}, error: {:?}", block, err);
    }
  }

  pub fn shares_accepted(&self, miner: &Miner, job: &Job) {
    let mut share_log = Point::new("valid_share");
    share_log.add_tag("address", IxValue::String(miner.login.to_owned()));
    share_log.add_field("value", IxValue::Integer(job.difficulty as i64));
    if let Err(err) = self.client.write_point(share_log, Some(Precision::Seconds), None) {
      warn!("Failed saving shares, error: {:?}", err);
    }
  }
}
