use std::sync::*;
use daemon_client::*;
use influx_db_client::*;
use serde_json::Value as SjValue;
use config::*;
use app::App;

#[derive(Debug)]
struct BlockShare {
  shares: u64,
  address: String,
  is_fee: bool,
}

pub struct Unlocker {
  app: Arc<App>,
}

impl Unlocker {
  pub fn new(app: Arc<App>) -> Unlocker {
    Unlocker {
      app,
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
      // TODO handle this a bit more gracefully, really all of the panics and unwrap()'s here should
      // be handled via Result<>
      err => panic!("Database error {:?}", err),
    }
  }

  pub fn refresh(&self) {
    self.process_blocks();
    self.process_payments();
  }

  pub fn process_blocks(&self) {
    let blocks = self.app.db.query(
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
          let header_for_height = self.app.daemon.get_block_header(height.as_u64().unwrap());
          match header_for_height {
            Ok(header) => {
              if &header.hash != block_id {
                // TODO maybe add a module to keep the code for writes in one place
                let mut orphaned = Point::new("block_status");
                orphaned.add_tag("block", Value::String(block_id.to_owned()));
                orphaned.add_field("status", Value::String("orphaned".to_owned()));
                let _ = self.app.db.write_point(orphaned, Some(Precision::Seconds), None).unwrap();
              }
              else if header.depth >= 60 {
                self.assign_balances(block_id, header.reward);
              }
              else {
                let mut unlocked = Point::new("block_progress");
                unlocked.add_tag("block", Value::String(block_id.to_owned()));
                unlocked.add_field("depth", Value::Integer(header.depth as i64));
                let _ = self.app.db.write_point(unlocked, Some(Precision::Seconds), None).unwrap();
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

  /// Appends donation fee shares, and returns the new total count of shares.  The pool fee is
  /// included in the returned total share count, but not appended to the share counts array, since
  /// there is no transaction needed to move funds from the pool to itself.
  pub fn append_fees(&self, share_counts: &mut Vec<BlockShare>) -> u64 {
    let miner_shares: u64 = share_counts.iter().map(|share| share.shares).sum();
    let dev_fee_percent: f64 = self.app.config.donations.iter().map(|donation| donation.percentage).sum();
    let total_fee_ratio: f64 = (self.app.config.pool_fee + dev_fee_percent) / 100.0;
    let miner_share_portion: f64 = 1.0 - total_fee_ratio;
    let total_shares = (miner_shares as f64 * (1.0 / miner_share_portion)).round() as u64;
    for &Donation { ref address, ref percentage } in &self.app.config.donations {
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
    let blocks = self.app.db.query(
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
    let shares= Unlocker::unwrap_query_results(self.app.db.query(
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
    let _ = self.app.db.write_point(unlocked, Some(Precision::Seconds), None).unwrap();
    let _ = self.app.db.write_points(share_inserts, Some(Precision::Seconds), None).unwrap();
  }

  pub fn process_payments(&self) {
    let payment_units_per_currency: f64 = 1e12;
    let owed_payments = self.app.db.query(
      &format!("SELECT * FROM (\
            SELECT sum(change) as sum_change \
            FROM miner_balance \
            GROUP BY address\
          ) WHERE sum_change > {}", self.app.config.min_payment * payment_units_per_currency),
      None,
    );
    let mut transfers = vec![];
    let mut balance_changes = Points::create_new(vec![]);
    for result in Unlocker::unwrap_query_results(owed_payments) {
      match result.as_slice() {
        &[SjValue::String(ref _timestamp), SjValue::String(ref address),
          SjValue::Number(ref sum_change)] => {
          if self.app.address_pattern.is_match(address) {
            let micro_denomination = self.app.config.payment_denomination * payment_units_per_currency;
            let payment = sum_change.as_u64().unwrap() % (micro_denomination as u64);
            transfers.push(Transfer {
              address: address.to_owned(),
              amount: payment,
            });

            let mut balance_insert = Point::new("miner_balance");
            balance_insert.add_tag("address", Value::String(address.to_owned()));
            balance_insert.add_field("change", Value::Integer(-1 * payment as i64));
            balance_changes.push(balance_insert);
          }
        },
        other => {
          println!("{:?}", other);
        }
      }
    }
    match self.app.daemon.transfer(&transfers) {
      Ok(result) => {
        // TODO need to make sure transfer addresses or valid are valid or this call will fail
        for mut balance_change in balance_changes.point.iter_mut() {
          balance_change.add_tag("payment_transaction", Value::String(result.tx_hash_list[0].to_owned()));
        }
        self.app.db.write_points(balance_changes, Some(Precision::Seconds), None).unwrap();
        for (fee, hash) in result.fee_list.iter().zip(result.tx_hash_list.iter()) {
          let mut payment_log = Point::new("pool_payment");
          payment_log.add_tag("transaction_hash", Value::String(hash.to_owned()));
          payment_log.add_field("fee", Value::Integer(*fee as i64));
          self.app.db.write_point(payment_log, Some(Precision::Seconds), None).unwrap();
        }
      },
      _ => println!("Failed to initiate transfer"),
    }
  }
}

#[test]
fn test_fee_percentages() {
  let fee_config = Config {
    hash_type: String::new(),
    influx_url: String::new(),
    daemon_url: String::new(),
    wallet_url: String::new(),
    payment_mixin: 0,
    min_payment: 0.0,
    payment_denomination: 0.0,
    pool_wallet: "pool".to_owned(),
    pool_fee: 10.0,
    donations: vec![Donation {
      address: "dev".to_owned(),
      percentage: 15.0,
    }],
    ports: Vec::new(),
  };
  let mut example_shares = vec![BlockShare {
    shares: 150000,
    address: "miner1".to_owned(),
    is_fee: false,
  }, BlockShare {
    shares: 50000,
    address: "miner2".to_owned(),
    is_fee: false,
  }];
  let unlocker = Unlocker::new(Arc::new(App::new(fee_config)));
  let total_shares = unlocker.append_fees(&mut example_shares);
  // Because the total fee percentage is 25% (an unrealistic but easy-to-reason-about number), 75%
  // of shares should go to the miners.
  assert_eq!(total_shares * 3 / 4, 150000 + 50000);
  // 90% of the shares should be allocated for transactions, the 10% pool fee in our scenario just
  // sits in the pool wallet.
  let distributed_shares: u64 = example_shares.iter().map(|share| share.shares).sum();
  assert_eq!(total_shares * 9 / 10, distributed_shares);
}