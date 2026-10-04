#!/usr/bin/env bash
set -euo pipefail
B=/root/rss-header-20261002
trap 'echo $? > "$B/gc-private-currentc170-idle-eden-v30-apps-driver.exit"' EXIT
export POLICY_ROOT=$B PATH=/opt/node-v26.5.1-linux-x64/bin:/root/.cargo/bin:$PATH
exec 9>/root/.perry-heavy.lock
flock 9
cd "$B"
python3 primary-idle-eden-v30-apps.py
