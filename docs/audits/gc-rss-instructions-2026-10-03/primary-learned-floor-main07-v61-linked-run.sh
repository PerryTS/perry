#!/usr/bin/env bash
set -euo pipefail
B=/root/rss-header-20261002
trap 'echo $? > "$B/gc-learned-floor-main07-v61-linked-driver.exit"' EXIT
export POLICY_ROOT=$B PATH=/opt/node-v26.5.1-linux-x64/bin:/root/.cargo/bin:$PATH
cd "$B"
python3 - <<'WAIT'
import pathlib,time,os
B=pathlib.Path('/root/rss-header-20261002')
for label,pid in [('gc-auto-lazy-rss-main07-v52',1769141),('gc-learned-floor-main07-v59-build',2022343)]:
 p=B/(label+'-driver.exit')
 while not p.exists():
  assert os.path.exists('/proc/'+str(pid)),('prerequisite stopped without terminal receipt',label,pid)
  time.sleep(30)
 assert p.read_text().strip()=='0',('prerequisite failed',label)
WAIT
exec 9>/root/.perry-heavy.lock
flock 9
python3 verify-lazy-rss-main07-v52-auto.py > "$B/lazy-rss-main07-v52-v61-prerequisite.log"
python3 verify-learned-floor-main07-v59-build.py > "$B/learned-floor-main07-v59-v61-prerequisite.log"
python3 primary-learned-floor-main07-v61-linked.py
