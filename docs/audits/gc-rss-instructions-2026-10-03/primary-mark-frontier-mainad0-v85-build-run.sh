#!/usr/bin/env bash
set -euo pipefail
B=/root/rss-header-20261002
trap 'echo $? > "$B/gc-mark-frontier-mainad0-v85-build-driver.exit"' EXIT
export PATH=/root/.cargo/bin:$PATH
cd "$B"
python3 - <<'PY'
import pathlib,time
b=pathlib.Path('/root/rss-header-20261002')
for name,pid in [('gc-mainad0-v80-ffi-events-driver.exit',2436323),('gc-auto-mainad0-v79-verifier-driver.exit',2518892)]:
 p=b/name
 while not p.exists():
  if pid:assert pathlib.Path(f'/proc/{pid}').exists(),'FFI wrapper stopped without terminal receipt'
  time.sleep(30)
 assert p.read_text().strip()=='0',name
PY
exec 9>/root/.perry-heavy.lock
flock 9
python3 primary-mark-frontier-mainad0-v85-build.py
python3 verify-mark-frontier-mainad0-v85-build.py
