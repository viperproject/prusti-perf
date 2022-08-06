#!/usr/bin/env bash
# Iterates over all BORS commits and performs a benchmark for each one
# Assumes that the `collector` executable is already built

set -euo pipefail

GIT_FETCH_INTERVAL_SECONDS=3600

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
LAST_GIT_FETCH_TIME=$(date +%s)
# git --no-pager log "$INITIAL_COMMIT" --author=bors --pretty=format:%H | while read -r SHA; do
while true; do
    CURRENT_TIME=$(date +%s)
    SHOULD_FETCH=$(($CURRENT_TIME - $LAST_GIT_FETCH_TIME > $GIT_FETCH_INTERVAL_SECONDS))
    if [ "$SHOULD_FETCH" == "1" ]; then
        git fetch
        LAST_GIT_FETCH_TIME=$(date +%s)
    fi
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
        ./x.py clean
        LAST_VIPER_TOOLCHAIN="$VIPER_TOOLCHAIN"
    fi
    cd "$PERF_DIR"
    set +e
    scripts/run_benchmark.sh
    if [ $? -ne 0 ]; then
        echo "Failure for SHA $SHA"
        PR=$(env PGPASSWORD=prusti psql -U prusti -h localhost -A -t -c \
            "SELECT pr FROM pull_request_build WHERE bors_sha = '$SHA' AND NOT complete")
        if [ ! -z "$PR" ]; then
            echo "PR was $PR"
            echo "The perf build for $SHA failed! You may want to retry" | scripts/post_github_comment.sh "$PR"
            env PGPASSWORD=prusti psql -U prusti -h localhost -c \
                "DELETE FROM pull_request_build WHERE pr=$PR"
        fi
    fi
    set -e
    cd "$PRUSTI_DIR"
done
