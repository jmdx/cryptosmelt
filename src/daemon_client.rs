use std::sync::atomic::*;
use std::sync::*;
use jsonrpc_core::*;
use reqwest;
use config::Config;

// TODO this will probably go in another file

pub struct DaemonClient {
  config: Arc<Config>,
}

impl DaemonClient {
  pub fn new(config: Arc<Config>) -> DaemonClient {
    DaemonClient {
      config,
    }
  }

  pub fn submit_block(&self, block: &str) -> reqwest::Result<Value> {
    self.call_daemon("submitblock", json!([block]))
  }

  pub fn get_block_template(&self, ) -> reqwest::Result<Value> {
    self.call_daemon("getblocktemplate", json!({
      "wallet_address": self.config.pool_wallet,
      "reserve_size": 8
    }))
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