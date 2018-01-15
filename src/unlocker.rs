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

  pub fn process_payments(&self, block_id: &str) {
    // TODO process the payments

    let mut unlocked = Point::new("block_status");
    unlocked.add_tag("block", Value::String(block_id.to_owned()));
    unlocked.add_field("status", Value::String("unlocked".to_owned()));
    let _ = self.db.write_point(unlocked, Some(Precision::Seconds), None).unwrap();
  }

  pub fn refresh(&self) {
    let blocks = self.db.query(
      "SELECT * FROM (\
            SELECT block, last(status) as last_status, height \
            FROM cryptosmelt.autogen.block_status \
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