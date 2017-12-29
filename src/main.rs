#[macro_use]
extern crate serde_derive;
extern crate toml;
extern crate jsonrpc_core;
extern crate jsonrpc_tcp_server;
extern crate concurrent_hashmap;
extern crate uuid;


mod config;
mod server;

fn main() {
  let _config = config::read_config();
  server::init(11337);
}
