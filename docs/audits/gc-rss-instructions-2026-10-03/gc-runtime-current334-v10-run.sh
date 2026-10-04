#!/usr/bin/env bash
set -euo pipefail
B=/root/rss-header-20261002
export POLICY_ROOT=$B PATH=/opt/node-v26.5.1-linux-x64/bin:/root/.cargo/bin:$PATH
cd "$B"
trap 'echo $? > "$B/gc-runtime-current334-v10-run.exit"' EXIT
bash gc-runtime-current334-v10-build.sh
python3 gc-runtime-current334-v10-linked.py
echo 0 > "$B/gc-runtime-current334-v10-linked.exit"
python3 gc-runtime-current334-v10-noauto.py
echo 0 > "$B/gc-runtime-current334-v10-noauto.exit"
python3 summarize-gc-comparison.py "$B/gc-current334-v10-noauto" --arms current334-base current334-gc --suite noauto --complete "$B/gc-runtime-current334-v10-noauto.exit"
python3 gc-runtime-current334-v10-auto-matrix.py
echo 0 > "$B/gc-runtime-current334-v10-auto.exit"
python3 summarize-gc-comparison.py "$B/gc-auto-current334-v10" --arms current334-base current334-gc --suite auto --fault-events --complete "$B/gc-runtime-current334-v10-auto.exit"
