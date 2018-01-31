#![feature(i128_type)]
#![feature(box_syntax)]
#![feature(slice_patterns)]
#![feature(plugin)]
#![plugin(rocket_codegen)]

#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate serde_json;
extern crate fern;
extern crate chrono;
#[macro_use]
extern crate log;
extern crate toml;
extern crate rocket;
extern crate rocket_contrib;
extern crate jsonrpc_core;
extern crate jsonrpc_tcp_server;
extern crate lru_time_cache;
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
extern crate diesel;
extern crate dotenv;
extern crate r2d2;
extern crate r2d2_diesel;


mod api;
mod app;
mod blocktemplate;
mod config;
mod cryptonote_utils;
mod cryptonightlite;
mod daemon_client;
mod db;
mod longkeccak;
mod miner;
mod stratum;
mod unlocker;
mod schema;
mod models;

use std::sync::Arc;
use app::App;

fn main() {
  let config = config::read_config();
  fern::Dispatch::new()
    .format(|out, message, record| {
      out.finish(format_args!(
        "{}[{}][{}] {}",
        chrono::Local::now().format("[%Y-%m-%d][%H:%M:%S]"),
        record.target(),
        record.level(),
        message
      ))
    })
    .level(config.log_level.parse().expect("Invalid log level"))
    .chain(std::io::stdout())
    .chain(fern::log_file(&config.log_file).expect("Invalid log file"))
    .apply().unwrap();
  let app_ref = Arc::new(App::new(config));
  api::init(app_ref.clone());
  stratum::init(app_ref);
}
