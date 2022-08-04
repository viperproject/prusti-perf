#!/usr/bin/env bash
# Iterates over all BORS commits and performs a benchmark for each one
# Assumes that the `collector` executable is already built

set -euo pipefail

if [ "$#" -gt 1 ]; then
    echo "Usage: scripts/generate_prusti_benchs.sh [FROM_COMMIT]"
    exit
fi

if [ "$#" -eq 1 ]; then
    INITIAL_COMMIT="$1"
else
    INITIAL_COMMIT="origin/master"
fi

source scripts/vars

LAST_VIPER_TOOLCHAIN=""

cd "$PRUSTI_DIR"
git fetch
# git --no-pager log "$INITIAL_COMMIT" --author=bors --pretty=format:%H | while read -r SHA; do
while true; do
    SHA=$(curl 'http://3.94.193.1:2346/perf/next_commit' | jq -r .commit.sha)
    if [ "$SHA" == "null" ]; then
        echo "No more commits, will check again in 60 seconds"
        sleep 60
        continue
    fi
    echo "Will run benchmarks for $SHA"
    git checkout "$SHA"
    VIPER_TOOLCHAIN=$(<viper-toolchain)
    if [ "$LAST_VIPER_TOOLCHAIN" != "$VIPER_TOOLCHAIN" ]; then
        echo "Using new viper toolchain $VIPER_TOOLCHAIN"
        ./x.py setup
        LAST_VIPER_TOOLCHAIN="$VIPER_TOOLCHAIN"
    fi
    cd "$PERF_DIR"
    scripts/run_benchmark.sh
    cd "$PRUSTI_DIR"
done
