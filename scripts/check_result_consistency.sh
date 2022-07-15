set -euo pipefail
NOW=$(date +%s)
scripts/run_benchmark.sh "first-$NOW"
scripts/run_benchmark.sh "second-$NOW"
echo "Results available at http://prusti-aws:2346/compare.html?start=first-$NOW&end=second-$NOW"
