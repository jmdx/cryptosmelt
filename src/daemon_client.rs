use std::sync::atomic::*;
use std::sync::*;
use jsonrpc_core::*;
use reqwest;
use config::Config;


#[derive(Deserialize)]
pub struct BlockHeader {
  pub hash: String,
  pub reward: u64,
  pub depth: u64,
}

pub struct DaemonClient {
  config: Arc<Config>,
}

/// Handles calls to the monero/aeon/etc. network, via the configured daemon_url.
impl DaemonClient {
  pub fn new(config: Arc<Config>) -> DaemonClient {
    DaemonClient {
      config,
    }
  }

  pub fn submit_block(&self, block: &str) -> reqwest::Result<Value> {
    self.call_daemon("submitblock", json!([block]))
  }

  pub fn get_block_template(&self) -> reqwest::Result<Value> {
    self.call_daemon("getblocktemplate", json!({
      "wallet_address": self.config.pool_wallet,
      "reserve_size": 8
    }))
  }

  pub fn get_block_header(&self, height: u64) -> reqwest::Result<BlockHeader> {
    match self.call_daemon("getblockheaderbyheight", json!({"height": height})) {
      Ok(value) => {
        // TODO don't unwrap() so much here
        let block_header = value.as_object().unwrap()
          .get("result").unwrap()
          .as_object().unwrap()
          .get("block_header").unwrap()
          .clone();
        Ok(serde_json::from_value(block_header).unwrap())
      },
      Err(err) => Err(err),
    }
  }

  fn call_daemon(&self, method: &str, params: Value) -> reqwest::Result<Value> {
    let map = json!({
      "jsonrpc": Value::String("2.0".to_owned()),
      "id": Value::String("0".to_owned()),
      "method": Value::String(method.to_owned()),
      "params": params,
    });
    let client = reqwest::Client::new();
    let mut res = client.post(&self.config.daemon_url)
      .json(&map)
      .send()?;
    res.json()
  }
}