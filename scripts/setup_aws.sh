#!/usr/bin/env bash

set -euo pipefail

sudo apt-get update -y
sudo apt-get install -y --no-install-recommends \
      ca-certificates \
      cmake \
      curl \
      default-jre \
      g++ \
      git \
      libc6-dev \
      libssl-dev \
      linux-tools-5.15.0-1011-aws \
      linux-tools-aws \
      linux-tools-common \
      make \
      mosh \
      pkg-config \
      unzip \
      silversearcher-ag \
      zlib1g-dev

curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

if ! grep perf_event_paranoid /etc/sysctl.conf > /dev/null; then
  sudo bash -c 'echo "kernel.perf_event_paranoid=-1" >> /etc/sysctl.conf'
  sudo sysctl -p /etc/sysctl.conf 
fi

cd "$HOME"

git clone git@github.com:viperproject/prusti-dev.git

if ! grep VIPER_HOME ~/.profile > /dev/null; then
  echo "export VIPER_HOME=$HOME/prusti-dev/viper_tools/backends" >> ~/.profile
fi

cd prusti-perf/collector
cargo build

