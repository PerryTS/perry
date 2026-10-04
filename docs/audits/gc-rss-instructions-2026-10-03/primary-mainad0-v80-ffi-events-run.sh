#!/usr/bin/env bash
set -euo pipefail
B=/root/rss-header-20261002
trap 'echo $? > "$B/gc-mainad0-v80-ffi-events-driver.exit"' EXIT
export PATH=/root/.cargo/bin:$PATH LLVM_SYS_221_PREFIX=/usr/lib/llvm-22
cd "$B"
python3 - <<'PY'
import pathlib,time,os
b=pathlib.Path('/root/rss-header-20261002');p=b/'gc-auto-mainad0-v79-driver.exit'
while not p.exists():
 assert pathlib.Path('/proc/2405797').exists(),'matrix stopped without terminal receipt'
 time.sleep(30)
assert p.read_text().strip()=='0' and (b/'gc-auto-mainad0-v79.exit').read_text().strip()=='0','matrix did not complete successfully'
PY
exec 9>/root/.perry-heavy.lock
flock 9
python3 - <<'PY'
import os
p='/root/rss-header-20261002';s=os.statvfs(p);assert s.f_bfree*s.f_frsize>=12*2**30,'need12GiB free before FFI/event build'
PY
python3 primary-mainad0-v80-ffi-events.py
