use std::sync::{Arc};
use config::*;
use daemon_client::*;
use influx_db_client::{Client};
use regex::Regex;

pub struct App {
  pub config: Config,
  pub db: Client,
  pub daemon: DaemonClient,
  pub address_pattern: Regex,
}

impl App {
  pub fn new(config: Config) -> App {
    let mut client = Client::default();
    let config_ref = Arc::new(config.clone());
    // TODO pull in the influx url from the config
    client.swith_database("cryptosmelt");
    let currency_prefix = config.pool_wallet.chars().next().unwrap();
    App {
      config,
      db: client,
      daemon: DaemonClient::new(config_ref),
      address_pattern: Regex::new(&(
        currency_prefix.to_string() + "[a-zA-Z0-9][123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz]{93}"
      )).unwrap()
    }
  }
}
