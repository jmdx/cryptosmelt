use std::fs::File;
use std::io::prelude::*;
use toml;

#[derive(Deserialize)]
pub struct Config {
  // TODO there will need to be both a wallet and a daemon address
  pub daemon_url: String,
  pub pool_wallet: String,
  pub ports: Vec<ServerConfig>,
}

#[derive(Deserialize)]
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
