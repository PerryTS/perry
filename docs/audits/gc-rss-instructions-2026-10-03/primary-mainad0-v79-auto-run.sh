#!/usr/bin/env bash
set -euo pipefail
B=/root/rss-header-20261002
trap 'echo $? > "$B/gc-auto-mainad0-v79-driver.exit"' EXIT
export POLICY_ROOT=$B PATH=/opt/node-v26.5.1-linux-x64/bin:/root/.cargo/bin:$PATH
cd "$B"
python3 - <<'WAIT'
import pathlib,time,os
B=pathlib.Path('/root/rss-header-20261002');p=B/'gc-mainad0-base-v77-build-driver.exit'
while not p.exists():
 assert os.path.exists('/proc/2384424'),'baseline stopped without terminal receipt'
 time.sleep(30)
assert p.read_text().strip()=='0','baseline build failed'
p=B/'rejected-auto-products-v78-offload-complete.json'
while not p.exists():time.sleep(30)
WAIT
exec 9>/root/.perry-heavy.lock
flock 9
python3 verify-mainad0-base-v77-build.py > "$B/gc-auto-mainad0-v79-base-prerequisite.log"
python3 verify-mainad0-once-v74-build.py > "$B/gc-auto-mainad0-v79-gc-prerequisite.log"
python3 primary-mainad0-v79-auto.py
