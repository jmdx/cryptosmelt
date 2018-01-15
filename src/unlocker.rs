use std::sync::*;
use daemon_client::DaemonClient;
use influx_db_client::*;
use serde_json::Value as SjValue;

pub struct Unlocker {
  daemon: Arc<DaemonClient>,
  db: Arc<Client>,
}

impl Unlocker {
  pub fn new(daemon: Arc<DaemonClient>, db: Arc<Client>) -> Unlocker {
    Unlocker {
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
    let share_counts: Vec<(u64, String)> = shares.iter().map(|share| {
      match share.as_slice() {
        &[SjValue::String(ref _timestamp), SjValue::String(ref address),
          SjValue::Number(ref shares)] => {
          (shares.as_u64().unwrap(), address.to_owned())
        },
        _ => (0, "".to_owned())
      }
    }).collect();
    let total_shares: u64 = share_counts.iter().map(|&(count, _)| count).sum();
    let mut share_inserts = Points::create_new(vec![]);
    for (share_count, address) in share_counts {
      let balance_change = (share_count as u128 * reward as u128) / total_shares as u128;
      let mut share_insert = Point::new("miner_balance");
      share_insert.add_tag("address", Value::String(address.to_owned()));
      share_insert.add_field("change", Value::Integer(balance_change as i64));
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
                self.process_payments(block_id);
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