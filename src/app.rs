use std::sync::{Arc};
use config::*;
use db::*;
use daemon_client::*;
use regex::Regex;

pub struct App {
  pub config: Config,
  pub db: DbAccess,
  pub daemon: DaemonClient,
  pub address_pattern: Regex,
}

impl App {
  pub fn new(config: Config) -> App {
    let config_ref = Arc::new(config.clone());
    let currency_prefix = config.pool_wallet.chars().next().unwrap();
    App {
      config,
      db: DbAccess::new(),
      daemon: DaemonClient::new(config_ref.clone()),
      address_pattern: Regex::new(&(
        currency_prefix.to_string() + "[a-zA-Z0-9][123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz]{93}"
      )).unwrap()
    }
  }

  pub fn total_fee(&self) -> f64 {
    let donation_fees: f64 = self.config.donations.iter()
      .map(|donation| donation.percentage)
      .sum();
    self.config.pool_fee + donation_fees
  }
}
