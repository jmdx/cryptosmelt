use std::sync::*;
use daemon_client::DaemonClient;
use influx_db_client::*;
use serde_json::Value as SjValue;
use config::*;

#[derive(Debug)]
struct BlockShare {
  shares: u64,
  address: String,
  is_fee: bool,
}

pub struct Unlocker {
  config: Arc<Config>,
  daemon: Arc<DaemonClient>,
  db: Arc<Client>,
}

impl Unlocker {
  pub fn new(config: Arc<Config>, daemon: Arc<DaemonClient>, db: Arc<Client>) -> Unlocker {
    Unlocker {
      config,
      daemon,
      db,
    }
  }

  fn unwrap_query_results(results: Result<Option<Vec<Node>>, Error>) -> Vec<Vec<SjValue>> {
    match results {
      Ok(Some(nodes)) => {
        nodes.iter().flat_map(|node| {
          node.series.iter().flat_map(|series| {
            series.iter().flat_map(|some_series| some_series.values.clone())
          })
        }).collect()
      }
      _ => vec![],
    }
  }

  /// Appends donation fee shares, and returns the new total count of shares.  The pool fee is
  /// included in the returned total share count, but not appended to the share counts array, since
  /// there is no transaction needed to move funds from the pool to itself.
  pub fn append_fees(&self, share_counts: &mut Vec<BlockShare>) -> u64 {
    let miner_shares: u64 = share_counts.iter().map(|share| share.shares).sum();
    let dev_fee_percent: f64 = self.config.donations.iter().map(|donation| donation.percentage).sum();
    let total_fee_ratio: f64 = (self.config.pool_fee + dev_fee_percent) / 100.0;
    let miner_share_portion: f64 = 1.0 - total_fee_ratio;
    let total_shares = (miner_shares as f64 * (1.0 / miner_share_portion)).round() as u64;
    for &Donation { ref address, ref percentage } in &self.config.donations {
      share_counts.push(BlockShare {
        shares: (total_shares as f64 * (percentage / 100.0)).round() as u64,
        address: address.to_owned(),
        is_fee: true
      });
    }
    total_shares
  }

  pub fn assign_balances(&self, block_id: &str, reward: u64) {
    // TODO process the payments
    let blocks = self.db.query(
      "SELECT last(*) FROM block_status WHERE status = 'unlocked'",
      None,
    );
    let results = Unlocker::unwrap_query_results(blocks);
    let mut time_filter = "".to_owned();
    for result in results {
      match result.as_slice() {
        &[SjValue::String(ref timestamp), ..] => {
          time_filter = format!("WHERE time > '{}'", timestamp);
        },
        _ => {}
      }
    }
    let shares= Unlocker::unwrap_query_results(self.db.query(
      &format!("SELECT address, sum FROM (SELECT sum(value) FROM valid_share {} GROUP BY address)", time_filter),
      None,
    ));
    let mut share_counts: Vec<BlockShare> = shares.iter().map(|share| {
      match share.as_slice() {
        &[SjValue::String(ref _timestamp), SjValue::String(ref address),
          SjValue::Number(ref shares)] => {
          BlockShare {
            shares: shares.as_u64().unwrap(),
            address: address.to_owned(),
            is_fee: false,
          }
        },
        _ => panic!("Bad response from database while preparing share calculation")
      }
    }).collect();
    let total_shares = self.append_fees(&mut share_counts);
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
    let _ = self.db.write_point(unlocked, Some(Precision::Seconds), None).unwrap();
    let _ = self.db.write_points(share_inserts, Some(Precision::Seconds), None).unwrap();
  }

  pub fn refresh(&self) {
    let blocks = self.db.query(
      "SELECT * FROM (\
            SELECT block, last(status) as last_status, height \
            FROM block_status \
            GROUP BY block\
          ) WHERE last_status = 'submitted'",
      None,
    );
    for result in Unlocker::unwrap_query_results(blocks) {
      match result.as_slice() {
        &[SjValue::String(ref timestamp), SjValue::String(ref _group),
          SjValue::String(ref block_id), SjValue::Number(ref height),
          SjValue::String(ref status)] => {
          let header_for_height = self.daemon.get_block_header(height.as_u64().unwrap());
          match header_for_height {
            Ok(header) => {
              if &header.hash != block_id {
                // TODO maybe add a module to keep the code for writes in one place
                let mut orphaned = Point::new("block_status");
                orphaned.add_tag("block", Value::String(block_id.to_owned()));
                orphaned.add_field("status", Value::String("orphaned".to_owned()));
                let _ = self.db.write_point(orphaned, Some(Precision::Seconds), None).unwrap();
              }
              else if header.depth >= 60 {
                self.assign_balances(block_id, header.reward);
              }
              else {
                let mut unlocked = Point::new("block_progress");
                unlocked.add_tag("block", Value::String(block_id.to_owned()));
                unlocked.add_field("depth", Value::Integer(header.depth as i64));
                let _ = self.db.write_point(unlocked, Some(Precision::Seconds), None).unwrap();
              }
            },
            // TODO log the cases below, probably want to find out a nice way of doing logs
            _ => {}
          }
        },
        _ => {}
      }
    }
  }
}

#[test]
fn test_fee_percentages() {
  let fee_config = Arc::new(Config {
    hash_type: String::new(),
    influx_url: String::new(),
    daemon_url: String::new(),
    pool_wallet: String::new(),
    pool_fee: 10.0,
    donations: vec![Donation {
      address: "dev".to_owned(),
      percentage: 15.0,
    }],
    ports: Vec::new(),
  });
  let mut example_shares = vec![BlockShare {
    shares: 150000,
    address: "miner1".to_owned(),
    is_fee: false,
  }, BlockShare {
    shares: 50000,
    address: "miner2".to_owned(),
    is_fee: false,
  }];
  let unlocker = Unlocker::new(
    fee_config.clone(),
    Arc::new(DaemonClient::new(fee_config.clone())),
    Arc::new(Default::default()),
  );
  let total_shares = unlocker.append_fees(&mut example_shares);
  // Because the total fee percentage is 25% (an unrealistic but easy-to-reason-about number), 75%
  // of shares should go to the miners.
  assert_eq!(total_shares * 3 / 4, 150000 + 50000);
  // 90% of the shares should be allocated for transactions, the 10% pool fee in our scenario just
  // sits in the pool wallet.
  let distributed_shares: u64 = example_shares.iter().map(|share| share.shares).sum();
  assert_eq!(total_shares * 9 / 10, distributed_shares);
}