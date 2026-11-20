#!/usr/bin/env python3
"""Stand-in for `perry`: `-o out` becomes a script replaying `// perry-*` lines."""
import os
import re
import sys

args = sys.argv[1:]
out = args[args.index("-o") + 1]
source = next(a for a in args if re.search(r"\.(m|c)?ts$|\.cjs$", a))
# `--target web` follows the source and names an HTML page; native output never does.
if ("--target" in args) != out.endswith(".html") or (
        "--target" in args and args[args.index(source) + 1:args.index("-o")] != ["--target", "web"]):
    print("error: --target web must follow the source and write an .html page")
    sys.exit(1)
text = open(source, encoding="utf-8").read()
if "// perry-compile-signal" in text:
    os.kill(os.getpid(), __import__("signal").SIGSEGV)
if "// perry-compile-fail" in text:
    print("error: cannot compile")
    sys.exit(1)
lines = re.findall(r"^// perry-out: ?(.*)$", text, re.M)
exit_code = re.search(r"^// perry-exit: (\d+)$", text, re.M)
sleep = re.search(r"^// perry-sleep: (\d+)$", text, re.M)
body = [
    "import os, signal, sys, time",
    "print(%r, end='')" % "".join(line + "\n" for line in lines),
    "print(os.environ.get('PARITY_PROBE', ''), end='')",
    "sys.stdout.flush()",
]
if sleep:
    body.append("time.sleep(%s)" % sleep.group(1))
if "// perry-signal" in text:
    body.append("os.kill(os.getpid(), signal.SIGSEGV)")
body.append("sys.exit(%s)" % (exit_code.group(1) if exit_code else 0))
with open(out, "w", encoding="utf-8") as handle:
    handle.write("#!%s\n%s\n" % (sys.executable, "\n".join(body)))
os.chmod(out, 0o755)
