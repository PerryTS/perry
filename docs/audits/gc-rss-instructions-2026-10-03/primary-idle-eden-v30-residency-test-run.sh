#!/usr/bin/env bash
set -euo pipefail
B=/root/rss-header-20261002
trap 'echo $? > "$B/gc-private-idle-eden-v30-residency-driver.exit"' EXIT
export PATH=/root/.cargo/bin:$PATH
exec 9>/root/.perry-heavy.lock
flock 9
cd "$B"
python3 primary-idle-eden-v30-residency-test.py
