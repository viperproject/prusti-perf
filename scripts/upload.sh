#!/usr/bin/env bash

set -euo pipefail

HOST=compute.zackg.me
REMOTE_USER=zgrannan

git diff > patch
scp patch "$HOST:/home/$REMOTE_USER/prusti-perf/patch"
ssh "$HOST" 'cd prusti-perf && git checkout . && git apply patch && rm patch'
rm patch
