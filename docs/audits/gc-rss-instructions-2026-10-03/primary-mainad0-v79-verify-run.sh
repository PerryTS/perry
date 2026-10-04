#!/usr/bin/env bash
set -euo pipefail
B=/root/rss-header-20261002
trap 'echo $? > "$B/gc-auto-mainad0-v79-verifier-driver.exit"' EXIT
cd "$B"
python3 - <<'PY'
import pathlib,time
b=pathlib.Path('/root/rss-header-20261002');p=b/'gc-auto-mainad0-v79-driver.exit'
while not p.exists():
 assert pathlib.Path('/proc/2405797').exists(),'matrix stopped without terminal receipt'
 time.sleep(30)
assert p.read_text().strip()=='0'
PY
exec 9>/root/.perry-heavy.lock
flock 9
python3 verify-mainad0-v79-auto.py
