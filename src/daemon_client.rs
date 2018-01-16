use std::sync::atomic::*;
use std::sync::*;
use jsonrpc_core::*;
use reqwest;
use config::Config;

#[derive(Serialize)]
pub struct Transfer {
  pub amount: u64,
  pub address: String,
}

#[derive(Deserialize)]
pub struct TransferResult {
  pub fee_list: Vec<u64>,
  pub tx_hash_list: Vec<String>,
}

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
    self.call_daemon(&self.config.daemon_url, "submitblock", json!([block]))
  }

  pub fn get_block_template(&self) -> reqwest::Result<Value> {
    self.call_daemon(&self.config.daemon_url, "getblocktemplate", json!({
      "wallet_address": self.config.pool_wallet,
      "reserve_size": 8
    }))
  }

  pub fn get_block_header(&self, height: u64) -> reqwest::Result<BlockHeader> {
    match self.call_daemon(&self.config.daemon_url, "getblockheaderbyheight", json!({"height": height})) {
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

  pub fn transfer(&self, transfers: &[Transfer]) -> reqwest::Result<TransferResult> {
    match self.call_daemon(&self.config.wallet_url, "transfer", json!({
      "destinations": transfers,
      "fee": 0, // The fee is specified, in the wallet API, but ignored
      "mixin": self.config.payment_mixin,
      "unlock_time": 0,
    })) {
      Ok(value) => {
        // TODO don't unwrap() so much here
        let transfer_result = value.as_object().unwrap()
          .get("result").unwrap()
          .clone();
        Ok(serde_json::from_value(transfer_result).unwrap())
      },
      Err(err) => Err(err),
    }
  }

  fn call_daemon(&self, url: &str, method: &str, params: Value) -> reqwest::Result<Value> {
    let map = json!({
      "jsonrpc": Value::String("2.0".to_owned()),
      "id": Value::String("0".to_owned()),
      "method": Value::String(method.to_owned()),
      "params": params,
    });
    let client = reqwest::Client::new();
    let mut res = client.post(url)
      .json(&map)
      .send()?;
    println!("{:?}", res);
    res.json()
  }
}