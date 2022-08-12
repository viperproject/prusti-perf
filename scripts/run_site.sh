#!/usr/bin/env bash

export RUST_BACKTRACE=1

cargo run --bin site --release -- postgresql://prusti:prusti@127.0.0.1
