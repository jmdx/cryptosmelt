use std::fs::File;
use std::io::prelude::*;
use toml;

#[derive(Deserialize)]
pub struct Config {
  pub hash_type: String,
  pub influx_url: String,
  // TODO there will need to be both a wallet and a daemon address
  pub daemon_url: String,
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
