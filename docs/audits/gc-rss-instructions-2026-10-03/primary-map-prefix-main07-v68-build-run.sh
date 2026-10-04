#!/usr/bin/env bash
set -euo pipefail
B=/root/rss-header-20261002
trap 'echo $? > "$B/gc-map-prefix-main07-v68-build-driver.exit"' EXIT
export PATH=/root/.cargo/bin:$PATH
exec 9>/root/.perry-heavy.lock
flock 9
cd "$B"
python3 cleanup-completed-once-v65-cache.py
python3 primary-map-prefix-main07-v68-build.py
python3 verify-map-prefix-main07-v68-build.py
