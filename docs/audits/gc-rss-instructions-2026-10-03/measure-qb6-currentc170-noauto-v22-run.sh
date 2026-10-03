#!/usr/bin/env bash
set -euo pipefail
B=/root/rss-header-20261002
trap 'echo $? > "$B/gc-currentc170-qb6-v22-noauto-driver.exit"' EXIT
export POLICY_ROOT=$B PATH=/opt/node-v26.5.1-linux-x64/bin:/root/.cargo/bin:$PATH
cd "$B"
python3 measure-qb6-currentc170-noauto-v22.py
echo 0 > gc-currentc170-qb6-v22-noauto.exit
python3 summarize-gc-comparison.py gc-currentc170-qb6-v22-noauto --arms currentc170-base currentc170-gc --suite noauto --complete gc-currentc170-qb6-v22-noauto.exit
