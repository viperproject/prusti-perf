#!/usr/bin/env bash

set -euo pipefail

sudo apt-get update -y

# Perf
sudo apt-get install -y \
  linux-tools-$(uname -r) \
  linux-cloud-tools-$(uname -r) \
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
      jq \
      make \
      mosh \
      pkg-config \
      postgresql-client-common \
      postgresql-client-14 \
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

sh <(curl -L https://nixos.org/nix/install) --daemon
cd ~/prusti-perf/z3nix

# Run in new shell so nix is available
bash -c 'nix-build -E "with import <nixpkgs> {}; callPackage ./default.nix {}"'

cd ~/prusti-perf/collector
cargo build



