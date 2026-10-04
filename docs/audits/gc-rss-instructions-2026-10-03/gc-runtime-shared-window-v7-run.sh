#!/usr/bin/env bash
set -euo pipefail
B=/root/rss-header-20261002
export POLICY_ROOT=$B PATH=/opt/node-v26.5.1-linux-x64/bin:/root/.cargo/bin:$PATH
cd "$B"
trap 'echo $? > "$B/gc-runtime-shared-window-v7-run.exit"' EXIT
bash gc-runtime-shared-window-v7-build.sh
python3 gc-runtime-shared-window-linked-v7.py
echo 0 > "$B/gc-runtime-shared-window-v7-linked.exit"
python3 gc-runtime-shared-window-noauto-v7.py
echo 0 > "$B/gc-runtime-shared-window-v7-noauto.exit"
python3 summarize-gc-comparison.py "$B/gc-shared-v7-noauto" --arms main69-base main69-shared --suite noauto --complete "$B/gc-runtime-shared-window-v7-noauto.exit"
python3 gc-runtime-shared-window-auto-matrix-v7.py
echo 0 > "$B/gc-runtime-shared-window-v7-auto.exit"
python3 summarize-gc-comparison.py "$B/gc-auto-shared-v7" --arms main69-base main69-shared --suite auto --fault-events --complete "$B/gc-runtime-shared-window-v7-auto.exit"
