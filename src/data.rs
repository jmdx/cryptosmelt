use reqwest;
use config::Config;
use std::sync::Arc;
use serde_json::*;

pub struct InfluxClient {
  url: String,
}

impl InfluxClient {
  pub fn new(config: Arc<Config>) -> InfluxClient {
    let instance = InfluxClient {
      url: config.influx_url.to_owned()
    };
    if let Err(err) = instance.query("CREATE DATABASE cryptosmelt") {
      // TODO if we can't establish a db connection that's probably a good case to panic
      println!("{}", err);
    }
    instance
  }

  pub fn write(&self, data: &str)
               -> reqwest::Result<String> {
    // TODO make sure everything gracefully handles influxdb being down
    let client = reqwest::Client::new();
    let mut res = client.post(&(self.url.to_owned() + "/write?db=cryptosmelt"))
      .body(data.to_owned())
      .send()?;
    res.text()
  }

  pub fn query(&self, query: &str)
                -> reqwest::Result<Value> {
    let map = json!({
      "q": query,
    });
    let client = reqwest::Client::new();
    let mut res = client.post(&(self.url.to_owned() + "/query"))
      .form(&map)
      .send()?;
    res.json()
  }
}