set -euo pipefail 

sudo docker stop perf-db
sudo docker rm perf-db
scripts/start_db.sh
