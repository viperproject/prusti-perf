#!/usr/bin/env bash

set -euo pipefail

sudo apt-get update -y

# Perf
sudo apt-get install -y \
  linux-tools-5.15.0-1011-aws \
  linux-tools-aws \
  linux-tools-common 

sudo apt-get install -y --no-install-recommends \
      ca-certificates \
      cmake \
      curl \
      default-jre \
      g++ \
      git \
      libc6-dev \
      libssl-dev \
      make \
      mosh \
      pkg-config \
      unzip \
      silversearcher-ag \
      zlib1g-dev

curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

curl -fsSL https://get.docker.com -o get-docker.sh
sudo sh get-docker.sh
rm get-docker.sh


if ! grep perf_event_paranoid /etc/sysctl.conf > /dev/null; then
  sudo bash -c 'echo "kernel.perf_event_paranoid=-1" >> /etc/sysctl.conf'
  sudo sysctl -p /etc/sysctl.conf 
fi

cd "$HOME"

git clone https://github.com/viperproject/prusti-dev.git

if ! grep VIPER_HOME ~/.profile > /dev/null; then
  echo "export VIPER_HOME=$HOME/prusti-dev/viper_tools/backends" >> ~/.profile
fi

sh <(curl -L https://nixos.org/nix/install) --daemon
cd ~/prusti-perf/z3nix
nix-build -E 'with import <nixpkgs> {}; callPackage ./default.nix {}'

cd ~/prusti-perf/collector
cargo build



