#!/usr/bin/env bash

set -euo pipefail

source scripts/vars

cd "$PRUSTI_DIR"
SHA=$(git rev-parse HEAD)
./x.py build --release
cd "$PERF_DIR"
Z3_EXE=$HOME/prusti-perf/z3nix/result/bin/z3 RUST_LOG=info PRUSTI_CHECK_OVERFLOWS=false $COLLECTOR bench_local \
    --id "commit:$SHA" \
    --cargo "$CARGO" \
    --profiles Check \
    --scenarios Full \
    --iterations "$NUM_ITERATIONS" \
    --db postgresql://prusti:prusti@127.0.0.1 \
    "$RUSTC"
