use std::fs::File;
use std::io::prelude::*;
use toml;

#[derive(Deserialize)]
pub struct Config {
  pub daemon: String,
}

pub fn read_config() -> Config {
  let mut f = File::open("config.toml").expect("config file not found");
  let mut contents = String::new();
  f.read_to_string(&mut contents)
    .expect("something went wrong reading the config file");
  toml::from_str(&contents).unwrap()
}
