set -e

echo "Installing postgres"
sudo apt install postgresql libpq-dev git libssl-dev pkg-config

mkdir ~/bin

echo "Installing rust"
curl https://sh.rustup.rs -sSf | sh
source $HOME/.cargo/env
rustup default nightly
cargo install diesel_cli --no-default-features --features postgres

echo "Starting postgres"
sudo service postgresql start

echo "Installing cryptosmelt"
git clone https://gitlab.com/jmdx/cryptosmelt ~/cryptosmelt
cd ~/cryptosmelt
sudo adduser cryptosmelt
echo DATABASE_URL=postgres://cryptosmelt:cryptosmeltpw@localhost/cryptosmelt > .env
cargo build --release
cp target/release/cryptosmelt ~/bin
sudo -u postgres psql -f scripts/psql_init.sql
sudo -u cryptosmelt diesel migration run

