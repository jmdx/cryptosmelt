#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate serde_json;
extern crate toml;
extern crate jsonrpc_core;
extern crate jsonrpc_tcp_server;
extern crate concurrent_hashmap;
extern crate uuid;
extern crate reqwest;


mod config;
mod server;

fn main() {
  let config = config::read_config();
  server::init(11337, config.daemon_url);
}
