use std::sync::*;
use jsonrpc_core::*;
use reqwest;
use std::result::Result;
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

  pub fn submit_block(&self, block: &str) -> Result<Value, String> {
    self.call_daemon(&self.config.daemon_url, "submitblock", json!([block]))
  }

  pub fn get_block_template(&self) -> Result<Value, String> {
    self.call_daemon(&self.config.daemon_url, "getblocktemplate", json!({
      "wallet_address": self.config.pool_wallet,
      "reserve_size": 8
    }))
  }

  pub fn get_block_header(&self, height: u64) -> Result<BlockHeader, String> {
    match self.call_daemon(&self.config.daemon_url, "getblockheaderbyheight", json!({"height": height})) {
      Ok(value) => {
        let bad_header_response = "Bad header response from daemon";
        let block_header = value.as_object()
          .ok_or(bad_header_response.to_owned())?
          .get("result")
          .ok_or(bad_header_response.to_owned())?
          .as_object()
          .ok_or(bad_header_response.to_owned())?
          .get("block_header")
          .ok_or(bad_header_response.to_owned())?
          .clone();
        serde_json::from_value(block_header)
          .map_err(|_| bad_header_response.to_owned())
      },
      Err(err) => Err(err),
    }
  }

  pub fn transfer(&self, transfers: &[Transfer]) -> Result<TransferResult, String> {
    match self.call_daemon(&self.config.wallet_url, "transfer", json!({
      "destinations": transfers,
      "fee": 0, // The fee is specified, in the wallet API, but ignored
      "mixin": self.config.payment_mixin,
      "unlock_time": 0,
    })) {
      Ok(value) => {
        let transfer_result = value.as_object()
          .ok_or("Bad transfer response from daemon".to_owned())?
          .get("result")
          .ok_or("Bad transfer response from daemon".to_owned())?
          .clone();
        serde_json::from_value(transfer_result)
          .map_err(|_| "Bad transfer response from daemon".to_owned())
      },
      Err(err) => Err(err),
    }
  }

  fn call_daemon(&self, url: &str, method: &str, params: Value) -> Result<Value, String> {
    let map = json!({
      "jsonrpc": Value::String("2.0".to_owned()),
      "id": Value::String("0".to_owned()),
      "method": Value::String(method.to_owned()),
      "params": params,
    });
    let client = reqwest::Client::new();
    let mut res = client.post(url)
      .json(&map)
      .send().map_err(|err| format!("Bad response from RPC server: {:?}", err))?;
    let json = res.json().map_err(|_| "Invalid JSON from RPC server".to_owned());
    match json {
      Ok(Value::Object(map)) => {
        if let Some(&Value::Object(ref err_object)) = map.get("error") {
          return Err(format!("Daemon produced error '{}', during {} on {}", match err_object.get("message") {
            Some(&Value::String(ref err_message)) => err_message.to_owned(),
            other => format!("{:?}", other)
          }, method, url));
        }
        Ok(Value::Object(map))
      }
      other => other,
    }
  }
}