#!/usr/bin/env bash
set -euo pipefail
B=/root/rss-header-20261002
export POLICY_ROOT=$B PATH=/opt/node-v26.5.1-linux-x64/bin:/root/.cargo/bin:$PATH
cd "$B"
trap 'echo $? > "$B/gc-runtime-window-v6r1-run.exit"' EXIT
python3 verify-gc-window-v6r1-inputs.py
python3 gc-runtime-window-noauto-v6r1.py
echo 0 > "$B/gc-runtime-window-v6r1-noauto.exit"
python3 summarize-gc-comparison.py gc-window-v6r1-noauto --arms main69-gc main69-window --suite noauto --complete gc-runtime-window-v6r1-noauto.exit
python3 gc-runtime-window-auto-matrix-v6r1.py
echo 0 > "$B/gc-runtime-window-v6r1-auto.exit"
python3 project-gc-window-comparison-v6r1.py
