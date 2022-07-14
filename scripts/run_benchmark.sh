#!/usr/bin/env bash

set -euo pipefail

source scripts/vars

cd "$PRUSTI_DIR"
SHA=$(git rev-parse HEAD)
./x.py build --release
cd "$PERF_DIR"
export Z3_EXE=$HOME/prusti-perf/z3nix/result/bin/z3 
export PRUSTI_CHECK_OVERFLOWS=false 

# Considerations for the number of parallel verifiers
#
# 1. Should appropriately represent the typical instantiation of this parameter,
#    which is # of cores + 1, to emulate typical performance (this would be relevant,
#    for example, in a change that made Prusti faster but Viper slower).
# 2. In principle it seems lower values would be better for obtaining consistent results
# 
# Note that we do not specify the resource limit here (another area of nondeterminism), since  
# it seeems that the resource bound actually causes an error in Heapsort for some reason.
# This is fixed in the latest version of Z3, but currently an old version is being used 
# due to https://github.com/viperproject/silicon/issues/535
export PRUSTI_EXTRA_VERIFIER_ARGS="--numberOfParallelVerifiers=4"

if [ "$#" -eq 1 ]; then
  BENCH_ID="$1"
else
  BENCH_ID="commit:$SHA"
fi

echo "Running benchmark $BENCH_ID"

$COLLECTOR bench_local \
    --id "$BENCH_ID" \
    --cargo "$CARGO" \
    --profiles Check \
    --scenarios Full \
    --iterations "$NUM_ITERATIONS" \
    --db postgresql://prusti:prusti@127.0.0.1 \
    "$RUSTC"
