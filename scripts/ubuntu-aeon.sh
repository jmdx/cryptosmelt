set -e


sudo apt install git cmake g++ libboost-all-dev libssl-dev pkg-config

mkdir ~/bin

echo "Installing aeon"
git clone https://github.com/aeonix/aeon ~/aeon
cd ~/aeon
make
cp build/release/src/aeond ~/bin
cp build/release/src/simplewallet ~/bin

