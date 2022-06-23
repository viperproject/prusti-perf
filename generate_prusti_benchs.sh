#!/usr/bin/env bash
# Iterates over all BORS commits and performs a benchmark for each one
# Assumes that the `collector` executable is already built

set -euo pipefail

PERF_DIR=$(pwd)
PRUSTI_DIR=$(readlink -f ../prusti-dev)
COLLECTOR=$PERF_DIR/target/debug/collector
CARGO=$(which cargo)
RUSTC=$PRUSTI_DIR/target/release/prusti-rustc

cd "$PRUSTI_DIR"
git --no-pager log origin/master --author=bors --pretty=format:%H | while read -r SHA; do
    git checkout "$SHA"
    ./x.py build --release
    cd "$PERF_DIR"
    RUST_LOG=info PRUSTI_CHECK_OVERFLOWS=false $COLLECTOR bench_local \
        --id "commit:$SHA" \
        --cargo "$CARGO" \
        --profiles Check \
        --scenarios Full \
        "$RUSTC"
    cd "$PRUSTI_DIR"
done
