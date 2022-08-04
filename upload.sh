#!/usr/bin/env bash

set -euo pipefail

git diff > patch
scp patch prusti-aws:/home/ubuntu/prusti-perf/patch
ssh prusti-aws 'cd prusti-perf && git checkout . && git apply patch && rm patch'
rm patch
