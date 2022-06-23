#!/usr/bin/env bash
# Iterates over all BORS commits and performs a benchmark for each one
# Assumes that the `collector` executable is already built

set -euo pipefail

if [ "$#" -gt 1 ]; then
    echo "Usage: ./generate_prusti_benchs.sh [FROM_COMMIT]"
    exit
fi

if [ "$#" -eq 1 ]; then
    INITIAL_COMMIT="$1"
else
    INITIAL_COMMIT="origin/master"
fi



PERF_DIR=$(pwd)
PRUSTI_DIR=$(readlink -f ../prusti-dev)
COLLECTOR=$PERF_DIR/target/debug/collector
CARGO=$(which cargo)
RUSTC=$PRUSTI_DIR/target/release/prusti-rustc
LAST_VIPER_TOOLCHAIN=""

cd "$PRUSTI_DIR"
git --no-pager log "$INITIAL_COMMIT" --author=bors --pretty=format:%H | while read -r SHA; do
    git checkout "$SHA"
    VIPER_TOOLCHAIN=$(<viper-toolchain)
    if [ "$LAST_VIPER_TOOLCHAIN" != "$VIPER_TOOLCHAIN" ]; then
        echo "Using new viper toolchain $VIPER_TOOLCHAIN"
        # ./x.py setup
        LAST_VIPER_TOOLCHAIN="$VIPER_TOOLCHAIN"
    fi
    # ./x.py build --release
    cd "$PERF_DIR"
    # RUST_LOG=info PRUSTI_CHECK_OVERFLOWS=false $COLLECTOR bench_local \
    #     --id "commit:$SHA" \
    #     --cargo "$CARGO" \
    #     --profiles Check \
    #     --scenarios Full \
    #     "$RUSTC"
    cd "$PRUSTI_DIR"
done
