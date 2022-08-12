#!/usr/bin/env bash

if ! command -v psql &> /dev/null; then
    echo 'Executable `psql` could not be found'
    echo "You may be able to install it with the command 'sudo apt install postgresql-client-common postgresql-client'"
    exit 1
fi

if ! command -v lsof &> /dev/null; then
    echo 'Executable `lsof` could not be found'
    echo "You may be able to install it with the command 'sudo apt install lsof'"
    exit 1
fi

set -euo pipefail

cd collector
cargo build
cd ..

HOST=http://localhost:2346
GIT_FETCH_INTERVAL_SECONDS=3600

# if [ "$#" -gt 1 ]; then
#     echo "Usage: scripts/generate_prusti_benchs.sh [FROM_COMMIT]"
#     exit
# fi

# if [ "$#" -eq 1 ]; then
#     INITIAL_COMMIT="$1"
# else
#     INITIAL_COMMIT="origin/master"
# fi

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
    SHA=$(curl "$HOST/perf/next_commit" | jq -r .commit.sha)
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
    BENCH_RESULT=$?

    if [ $BENCH_RESULT -eq 3 ]; then
        echo "Unexpected error running benchmarks, aborting"
        exit 1
    fi
    if [ $BENCH_RESULT -ne 0 ]; then
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
