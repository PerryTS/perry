#!/usr/bin/env bash
set -euo pipefail
B=/root/rss-header-20261002
export POLICY_ROOT=$B PATH=/opt/node-v26.5.1-linux-x64/bin:/root/.cargo/bin:$PATH
cd "$B"
trap 'echo $? > "$B/gc-runtime-currente532-v12-run.exit"' EXIT
bash gc-runtime-currente532-v12-build.sh
python3 gc-runtime-currente532-v12-linked.py
echo 0 > "$B/gc-runtime-currente532-v12-linked.exit"
python3 gc-runtime-currente532-v12-noauto.py
echo 0 > "$B/gc-runtime-currente532-v12-noauto.exit"
python3 summarize-gc-comparison.py "$B/gc-currente532-v12-noauto" --arms currente532-base currente532-gc --suite noauto --complete "$B/gc-runtime-currente532-v12-noauto.exit"
python3 gc-runtime-currente532-v12-auto-matrix.py
echo 0 > "$B/gc-runtime-currente532-v12-auto.exit"
python3 summarize-gc-comparison.py "$B/gc-auto-currente532-v12" --arms currente532-base currente532-gc --suite auto --fault-events --complete "$B/gc-runtime-currente532-v12-auto.exit"
