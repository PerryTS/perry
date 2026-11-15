#!/usr/bin/env python3
"""Stand-in for `node`: replays `// node-out:` / `// node-exit:` / `// node-signal` lines."""
import os
import re
import signal
import sys

if sys.argv[1:] == ["--version"]:
    print("v1.0.0")
    sys.exit(0)
# The web host runs a page; the stand-in page is the replay script fake_perry wrote.
if sys.argv[1].endswith("parity_web_host.cjs"):
    os.execv(sys.executable, [sys.executable, *sys.argv[2:]])
source = next(a for a in sys.argv[1:] if re.search(r"\.(m|c)?ts$|\.cjs$", a))
text = open(source, encoding="utf-8").read()
for line in re.findall(r"^// node-out: ?(.*)$", text, re.M):
    print(line)
sys.stdout.flush()
if "// node-signal" in text:
    os.kill(os.getpid(), signal.SIGSEGV)
exit_code = re.search(r"^// node-exit: (\d+)$", text, re.M)
sys.exit(int(exit_code.group(1)) if exit_code else 0)
