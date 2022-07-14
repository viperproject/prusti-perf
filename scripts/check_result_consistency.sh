set -euo pipefail
NOW=$(date +%s)
scripts/run_benchmark.sh "first-$NOW"
scripts/run_benchmark.sh "second-$NOW"
