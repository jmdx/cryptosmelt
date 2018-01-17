use std::fs::File;
use std::io::prelude::*;
use toml;

#[derive(Clone, Deserialize)]
pub struct Config {
  pub hash_type: String,
  pub log_level: String,
  pub log_file: String,
  pub influx_url: String,
  pub daemon_url: String,
  pub wallet_url: String,
  pub payment_mixin: u64,
  pub min_payment: f64,
  pub payment_denomination: f64,
  pub pool_wallet: String,
  pub pool_fee: f64,
  pub donations: Vec<Donation>,
  pub ports: Vec<ServerConfig>,
}

#[derive(Clone, Deserialize)]
pub struct Donation {
  pub address: String,
  pub percentage: f64,
}

#[derive(Clone, Deserialize)]
pub struct ServerConfig {
  pub port: u16,
  pub difficulty: u64,
}

pub fn read_config() -> Config {
  let mut f = File::open("config.toml").expect("config file not found");
  let mut contents = String::new();
  f.read_to_string(&mut contents)
    .expect("something went wrong reading the config file");
  toml::from_str(&contents).unwrap()
}
