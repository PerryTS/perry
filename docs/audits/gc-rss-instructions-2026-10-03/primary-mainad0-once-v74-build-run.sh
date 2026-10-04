#!/usr/bin/env bash
set -euo pipefail
B=/root/rss-header-20261002
trap 'echo $? > "$B/gc-mainad0-once-v74-build-driver.exit"' EXIT
export PATH=/root/.cargo/bin:$PATH
exec 9>/root/.perry-heavy.lock
flock 9
cd "$B"
python3 primary-mainad0-once-v74-build.py
python3 verify-mainad0-once-v74-build.py
