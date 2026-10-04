#!/usr/bin/env bash
set -euo pipefail
B=/root/rss-header-20261002
trap 'echo $? > "$B/gc-map-prefix-main07-v70-linked-driver.exit"' EXIT
export POLICY_ROOT=$B PATH=/opt/node-v26.5.1-linux-x64/bin:/root/.cargo/bin:$PATH
cd "$B"
python3 - <<'WAIT'
import pathlib,time,os
B=pathlib.Path('/root/rss-header-20261002'); p=B/'gc-map-prefix-main07-v68-build-driver.exit'
while not p.exists():
 assert os.path.exists('/proc/2221063'),'prerequisite stopped without terminal receipt'
 time.sleep(30)
assert p.read_text().strip()=='0','private runtime build failed'
WAIT
exec 9>/root/.perry-heavy.lock
flock 9
python3 verify-map-prefix-main07-v68-build.py > "$B/gc-map-prefix-main07-v70-linked-prerequisite.log"
python3 primary-map-prefix-main07-v70-linked.py
