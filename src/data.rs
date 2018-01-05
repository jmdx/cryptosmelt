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
      println!("{}", err);
    }
    instance
  }

  // TODO this will probably go in another file
  pub fn write(&self, data: &str)
               -> reqwest::Result<String> {
    let client = reqwest::Client::new();
    let mut res = client.post(&(self.url.to_owned() + "/write?db=cryptosmelt"))
      .body(data.to_owned())
      .send()?;
    println!("{:?}", res);
    res.text()
  }

  // TODO this will probably go in another file
  pub fn query(&self, query: &str)
                -> reqwest::Result<Value> {
    let map = json!({
      "q": query,
    });
    let client = reqwest::Client::new();
    let mut res = client.post(&(self.url.to_owned() + "/query"))
      .form(&map)
      .send()?;
    println!("{:?}", res);
    res.json()
  }
}