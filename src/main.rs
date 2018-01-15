#![feature(i128_type)]
#![feature(box_syntax)]
#![feature(slice_patterns)]

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
extern crate mithril;
extern crate groestl;
extern crate blake;
extern crate jhffi;
extern crate skeinffi;
extern crate regex;
#[macro_use]
extern crate influx_db_client;


mod config;
mod server;
mod cryptonightlite;
mod longkeccak;
mod blocktemplate;
mod daemon_client;
mod unlocker;

fn main() {
  let config = config::read_config();
  server::init(config);
}
