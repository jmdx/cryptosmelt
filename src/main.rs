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
extern crate schedule_recv;
extern crate bytes;
extern crate num_bigint;
extern crate num_integer;


mod config;
mod server;

fn main() {
  let config = config::read_config();
  server::init(11337, config.daemon_url, config.pool_wallet);
}
