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
      },
      _ => vec![],
    }
  }

  pub fn refresh(&self) {
    let blocks = self.db.query(
      "SELECT block, last(status) FROM cryptosmelt.autogen.block_status GROUP BY block",
      None
    );
    for result in Unlocker::unwrap_query_results(blocks) {
      // println!("{:?}", result);
    }
  }
}