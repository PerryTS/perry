#!/usr/bin/env bash
set -euo pipefail
B=/root/rss-header-20261002
trap 'echo $? > "$B/gc-auto-currentc170-qb6-v22-driver.exit"' EXIT
export POLICY_ROOT=$B PATH=/opt/node-v26.5.1-linux-x64/bin:/root/.cargo/bin:$PATH
cd "$B"
python3 measure-qb6-currentc170-auto-v22.py
python3 summarize-gc-comparison.py gc-auto-currentc170-qb6-v22 --arms currentc170-base currentc170-gc --suite auto --fault-events --complete gc-auto-currentc170-qb6-v22.exit
