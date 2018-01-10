# Cryptosmelt
Cryptonote and Cryptonote-Lite pool software, written in Rust and using InfluxDB as the data store.
__This does not have any payment/reward system in place, so is not ready for use.__

# Dependencies
- Rust (nightly version)
- InfluxDB 1.4

# Installation

Install the Rust nightly version.  This is easiest to do via [rustup](https://www.rustup.rs/):

```
curl https://sh.rustup.rs -sSf | sh
rustup install nightly
```

*Nightly is needed because of the dependency upon [mithril's](https://github.com/Ragnaroek/mithril) Cryptonight hash implementation.*

Then install InfluxDB 1.4 via the instructions [here.](https://docs.influxdata.com/influxdb/v1.4/introduction/installation/)
At the time of writing, Windows binaries are not listed in influxdb's documentation, but can be found [on their download page.](https://portal.influxdata.com/downloads)

Finally, checkout this repo and enter your pool wallet address (currently Aeon-only) as `pool_wallet` in `config.toml`.  Then execute `cargo run` and the server will listen on the ports configured in that file.

# Recommended tools

- Intellij has a Rust plugin that is already excellent: https://intellij-rust.github.io

