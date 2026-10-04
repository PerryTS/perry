#!/usr/bin/env bash
set -euo pipefail
B=/root/rss-header-20261002
trap 'echo $? > "$B/gc-mainad0-once-v76r1-linked-driver.exit"' EXIT
export POLICY_ROOT=$B PATH=/opt/node-v26.5.1-linux-x64/bin:/root/.cargo/bin:$PATH
exec 9>/root/.perry-heavy.lock
flock 9
cd "$B"
python3 verify-mainad0-once-v74-build.py > "$B/gc-mainad0-once-v76r1-linked-prerequisite.log"
python3 primary-mainad0-once-v76r1-linked.py
