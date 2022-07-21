#!/usr/bin/env bash

set -euo pipefail

source scripts/vars

# Note that if USE_SERVER=true, then `perf` will not include any metrics for the backend
# This makes the results more consistent across runs, but fails to account for improvements
# due to more performant encodings to viper
USE_SERVER=true

cd "$PRUSTI_DIR"
SHA=$(git rev-parse HEAD)
./x.py build --release

export LD_LIBRARY_PATH=/usr/lib/jvm/default-java/lib/server
export Z3_EXE=$HOME/prusti-perf/z3nix/result/bin/z3
export PRUSTI_ENABLE_CACHE=false
export PRUSTI_CHECK_OVERFLOWS=false 
# export PRUSTI_EXTRA_VERIFIER_ARGS="--proverEnableResourceBounds"

if [ "$USE_SERVER" == "true" ];  then
  PRUSTI_SERVER="$PRUSTI_DIR/target/release/prusti-server-driver"
  PRUSTI_SERVER_PORT=12345
  if lsof -Pi ":$PRUSTI_SERVER_PORT" -sTCP:LISTEN -t >/dev/null ; then
     echo "Prusti server already running, but it shouldn't be!"
     exit 1
  fi
  export PRUSTI_SERVER_ADDRESS="localhost:$PRUSTI_SERVER_PORT"
  $PRUSTI_SERVER --port "$PRUSTI_SERVER_PORT"&
  SERVER_PID=$!
  sleep 2
  trap "kill $SERVER_PID" EXIT
fi

cd "$PERF_DIR"


if [ "$#" -eq 1 ]; then
  BENCH_ID="$1"
else
  BENCH_ID="commit:$SHA"
fi

# Warmup is only useful if server is used
if [ "$USE_SERVER" == "true" ];  then
  WARMUP_ID="warmup-$(date +%s)"
  echo "Running warmup $WARMUP_ID"
  export BENCH_PERF_ITERATIONS=1
  $COLLECTOR bench_local \
      --id "warmup-$(date +%s)" \
      --cargo "$CARGO" \
      --profiles Check \
      --scenarios Full \
      --db postgresql://prusti:prusti@127.0.0.1 \
      "$RUSTC"
fi

echo "Running benchmark $BENCH_ID"
export BENCH_PERF_ITERATIONS=3
$COLLECTOR bench_local \
    --id "$BENCH_ID" \
    --cargo "$CARGO" \
    --profiles Check \
    --scenarios Full \
    --iterations "$NUM_ITERATIONS" \
    --db postgresql://prusti:prusti@127.0.0.1 \
    "$RUSTC"

curl -XPOST localhost:2346/perf/onpush || echo "Unable to refresh perf site (probably it is not running)"
