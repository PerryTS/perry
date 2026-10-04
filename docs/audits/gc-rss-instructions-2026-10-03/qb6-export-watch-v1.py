"""Continuously copy only our qb6 artifacts, stopping before its rental deadline."""
from pathlib import Path
from datetime import datetime, timezone
import json
import subprocess
import time

B = Path(__file__).resolve().parent
D = B / 'qb6-currente532-build-v1'
D.mkdir(exist_ok=True)
deadline = datetime(2026, 10, 3, 17, 55, tzinfo=timezone.utc).timestamp()
attempts = []
while time.time() < deadline:
    r = subprocess.run(['rsync', '-az', '--partial',
                        'root@185.191.117.52:/root/rss-gc-build-20261003/export/', str(D) + '/'],
                       capture_output=True, text=True, timeout=300)
    attempts.append(dict(utc=time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime()),
                         rc=r.returncode, stderr=r.stderr))
    (B / 'qb6-export-watch-v1-record.json').write_text(json.dumps(attempts, indent=2) + '\n')
    receipt = D / 'build-driver.exit'
    if r.returncode == 0 and receipt.exists():
        print('export complete; build receipt', receipt.read_text().strip(), flush=True)
        break
    time.sleep(45)
else:
    print('export deadline reached; preserved artifacts copied so far', flush=True)
