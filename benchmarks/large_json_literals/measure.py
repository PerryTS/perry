"""Fresh-cache, standalone literal measurements; run on the Linux build host.

Usage: measure.py COMPILER_DIR SOURCE RESULT_DIR [--link] [--fast-emit N]
The result directory must not exist. Logs, GNU time, section sizes and symbol
sizes are retained alongside summary.json. No application graph is compiled.
"""

import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("compiler", type=Path)
    parser.add_argument("source", type=Path)
    parser.add_argument("result", type=Path)
    parser.add_argument("--link", action="store_true")
    parser.add_argument("--fast-emit", type=int)
    parser.add_argument("--profile", action="store_true")
    parser.add_argument("--timeout", type=int, default=600)
    args = parser.parse_args()
    if os.uname().sysname != "Linux":
        parser.error("build-host measurements only")
    # Inspect only Perry processes: the guard cannot match this script or its
    # shell command line. Another lane's full application compile gets priority.
    running = subprocess.run(
        ["ps", "-C", "perry", "-o", "args="], capture_output=True, text=True
    ).stdout
    if re.search(r"\bcompile src/index\.ts\b", running):
        parser.error("a full OpenCode compile is running; wait for it to exit")
    compiler = args.compiler.resolve()
    source = args.source.resolve()
    result = args.result.resolve()
    result.mkdir(parents=True, exist_ok=False)
    (result / "perry.json").write_text("{}\n")
    env = os.environ.copy()
    env.update(PERRY_RUNTIME_DIR=str(compiler), PERRY_CODEGEN_PROGRESS="all",
               PERRY_CODEGEN_UNIT_TIMINGS="1", PERRY_DEBUG_SYMBOLS="1")
    if args.fast_emit is not None:
        env["PERRY_LL_FAST_EMIT_MAX_INSTRS"] = str(args.fast_emit)
    output = result / ("app" if args.link else "module.o")
    command = ["/usr/bin/time", "-v", "-o", str(result / "time.txt"),
               "timeout", str(args.timeout), str(compiler / "perry"), "compile",
               str(source), "--no-auto-optimize", "--output", str(output),
               "--cache-dir", str(result / "cache")]
    if not args.link:
        command.append("--no-link")
    if args.profile:
        command = ["perf", "record", "-F", "99", "--call-graph", "dwarf,8192",
                   "-o", str(result / "perf.data"), "--"] + command
    (result / "command.json").write_text(json.dumps(command, indent=2) + "\n")
    started = time.monotonic()
    with (result / "compile.log").open("w") as log:
        process = subprocess.Popen(command, cwd=result, env=env,
                                   stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                                   text=True, errors="replace", bufsize=1)
        for line in process.stdout:
            log.write(f"{time.monotonic() - started:9.3f} {line}")
            log.flush()
        status = process.wait()
    summary = dict(source=str(source), compiler=str(compiler), status=status,
                   wall_seconds=round(time.monotonic() - started, 3))
    if (result / "time.txt").exists():
        timing = (result / "time.txt").read_text()
        rss = re.search(r"Maximum resident set size \(kbytes\): (\d+)", timing)
        summary["rss_kib"] = int(rss[1]) if rss else None
    if status == 0:
        sections = subprocess.check_output(["size", "-A", str(output)], text=True)
        (result / "sections.txt").write_text(sections)
        summary["text_bytes"] = sum(
            int(match[1]) for match in
            re.finditer(r"^\.text\S*\s+(\d+)", sections, re.MULTILINE)
        )
        symbols = subprocess.check_output(
            ["nm", "--print-size", "--size-sort", str(output)], text=True
        )
        (result / "symbols.txt").write_text(symbols)
    (result / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
    print(json.dumps(summary), flush=True)
    raise SystemExit(status)


if __name__ == "__main__":
    main()
