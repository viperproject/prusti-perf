#!/usr/bin/env bash

set -euo pipefail

source scripts/vars

cd "$PRUSTI_DIR"
SHA=$(git rev-parse HEAD)
./x.py build --release
cd "$PERF_DIR"
export Z3_EXE=$HOME/prusti-perf/z3nix/result/bin/z3 
export PRUSTI_CHECK_OVERFLOWS=false 
export PRUSTI_EXTRA_VERIFIER_ARGS="--proverEnableResourceBounds"
$COLLECTOR bench_local \
    --id "commit:$SHA" \
    --cargo "$CARGO" \
    --profiles Check \
    --scenarios Full \
    --iterations "$NUM_ITERATIONS" \
    --db postgresql://prusti:prusti@127.0.0.1 \
    "$RUSTC"
