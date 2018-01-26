use std::sync::*;
use daemon_client::*;
use serde_json::Value as SjValue;
use config::*;
use db::*;
use app::App;

pub struct Unlocker {
  app: Arc<App>,
}

impl Unlocker {
  pub fn new(app: Arc<App>) -> Unlocker {
    Unlocker {
      app,
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
    );
    for result in blocks {
      match result.as_slice() {
        &[SjValue::String(ref _timestamp), SjValue::String(ref _group),
          SjValue::String(ref block_id), SjValue::Number(ref height),
          SjValue::String(ref _status)] => {
          let header_for_height = self.app.daemon.get_block_header(height.as_u64().unwrap());
          match header_for_height {
            Ok(header) => {
              if &header.hash != block_id {
                self.app.db.block_status(block_id, "orphaned");
              }
              else if header.depth >= 60 {
                self.assign_balances(block_id, header.reward);
              }
              else {
                self.app.db.block_progress(block_id, header.depth);
              }
            },
            Err(err) => {
              warn!("Unexpected result from daemon: {:?}", err);
            }
          }
        },
        err => {
          warn!("Unexpected block query result: {:?}", err);
        }
      }
    }
  }

  /// Appends donation fee shares, and returns the new total count of shares.  The pool fee is
  /// included in the returned total share count, but not appended to the share counts array, since
  /// there is no transaction needed to move funds from the pool to itself.
  fn append_fees(&self, share_counts: &mut Vec<BlockShare>) -> u64 {
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
    let results = self.app.db.query(
      "SELECT * FROM block_status WHERE status = 'unlocked' ORDER BY time DESC LIMIT 1",
    );
    debug!("Assigning balances...");
    let mut time_filter = "".to_owned();
    for result in results {
      match result.as_slice() {
        &[SjValue::String(ref timestamp), ..] => {
          debug!("last block at {}", timestamp);
          time_filter = format!("WHERE time > '{}'", timestamp);
        },
        _ => {}
      }
    }
    let shares = self.app.db.query(
      &format!(
        "SELECT address, sum FROM (SELECT sum(value) FROM valid_share {} GROUP BY address) \
         WHERE address <> ''",
        time_filter
      ),
    );
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
    self.app.db.distribute_balances(reward, block_id, share_counts, total_shares);
  }

  pub fn process_payments(&self) {
    let payment_units_per_currency: f64 = 1e12;
    let owed_payments = self.app.db.query(
      &format!("SELECT * FROM (\
            SELECT sum(change) as sum_change \
            FROM miner_balance \
            GROUP BY address\
          ) WHERE sum_change > {}", self.app.config.min_payment * payment_units_per_currency),
    );
    let mut transfers = vec![];
    for result in owed_payments {
      match result.as_slice() {
        &[SjValue::String(ref _timestamp), SjValue::String(ref address),
          SjValue::Number(ref sum_change)] => {
          if self.app.address_pattern.is_match(address) {
            let micro_denomination = self.app.config.payment_denomination * payment_units_per_currency;
            let mut payment = sum_change.as_u64().unwrap();
            payment -= payment % (micro_denomination as u64);
            info!("Sum change {}, payment {}, denomination {}", sum_change, payment, micro_denomination);
            if payment > 0 {
              transfers.push(Transfer {
                address: address.to_owned(),
                amount: payment,
              });
            }
          }
        },
        other => {
          warn!("Received invalid result from miner_balance series: {:?}", other);
        }
      }
    }
    if transfers.len() == 0 {
      return;
    }
    info!("Transfers: {:?}", &transfers);
    if self.app.db.is_connected() {
      // It's important to check that we have a connection before transferring, since not having
      // a DB connection after a transfer is a dangerous case.  There is still the chance that we
      // could lose connection during the transfer, but this is as close as we can get to an atomic
      // transaction between our database and the daemon.
      match self.app.daemon.transfer(&transfers) {
        Ok(result) => {
          self.app.db.log_transfers(&transfers, &result.tx_hash, result.fee);
        },
        Err(err) => error!("Failed to initiate transfer: {:?}", err),
      }
    }
    else {
      warn!("Miners have payable balances, but the connection was lost while computing them.")
    }
  }
}

#[cfg(test)]
mod tests {
  use unlocker::*;

  #[test]
  fn test_fee_percentages() {
    let fee_config = Config {
      hash_type: String::new(),
      log_level: String::new(),
      log_file: String::new(),
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
}