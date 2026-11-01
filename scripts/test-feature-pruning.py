#!/usr/bin/env python3
"""Bounded native regressions for #13659, using speed-oriented build flags.

PERRY_BIN selects a built compiler; PERRY_RUNTIME_DIR pins its runtime archives.
Pass --baseline COMPILER for identical-path stripped file/__text size comparisons.
"""
import argparse
import json
import os
from pathlib import Path
import shutil
import signal
import statistics
import subprocess
import tempfile
import time


def run(command, **kwargs):
    return subprocess.run(command, check=True, text=True, capture_output=True,
                          timeout=kwargs.pop("timeout", 120), **kwargs)


def compile_source(compiler, directory, name, source):
    entry = directory / (name + ".ts")
    output = directory / name
    entry.write_text(source)
    env = dict(os.environ, PERRY_NO_CACHE="1", PERRY_KEEP_SYMBOLS="1",
               PERRY_LL_OPT_LEVEL="s", PERRY_SPECIALIZED_ABI="1")
    run([compiler, "compile", str(entry), "-o", str(output), "--no-cache",
         "--function-source", "header", "--strict-eval", "--strict-dynamic-import",
         "--strict-unimplemented"], env=env, timeout=300)
    return output


def has_regex(binary):
    return "js_runtime_install_regex_engine" in run(["nm", str(binary)]).stdout


def size(binary):
    if os.uname().sysname == "Darwin":
        sections = run(["size", "-m", str(binary)]).stdout.splitlines()
        text = next(int(line.split()[2]) for line in sections if line.strip().startswith("Section __text:"))
        run(["strip", str(binary)])
        run(["codesign", "--force", "--sign", "-", str(binary)])
    else:
        sections = run(["size", "-A", str(binary)]).stdout.splitlines()
        text = next(int(line.split()[1]) for line in sections if line.startswith(".text "))
        run(["strip", str(binary)])
    return {"file_bytes": binary.stat().st_size, "text_bytes": text}


def timed_runs(binary, count):
    samples = []
    for _ in range(count):
        start = time.perf_counter()
        run([str(binary)], timeout=10)
        samples.append((time.perf_counter() - start) * 1000)
    return round(statistics.median(samples), 3)


SOURCES = {
    "control": 'console.log("ok");',
    "signal": 'process.on("SIGINT", () => {}); console.log("ok");',
    "search": 'class Probe { search(): number { return 1; } } console.log(new Probe().search());',
    "find": 'class Probe { findValue(): number { return 1; } } console.log(new Probe().findValue());',
    "match": 'class Probe { match(): number { return 1; } } console.log(new Probe().match());',
    "regex": 'console.log("abc".search("b"));',
    "unknown": 'function search(value: any) { return value.search("b"); } console.log(search("abc"));',
    "steady": '''process.on("SIGINT", () => {});
function work(seed: number, iterations: number): number {
    let x = seed;
    for (let i = 0; i < iterations; i++) x = Math.imul(x, 1664525) + 1013904223 | 0;
    return x;
}
console.log(work(Number(process.argv[2] || "1"), Number(process.argv[3] || "10000000")));''',
}


def check_signals(compiler, directory):
    # No process namespace escape or IPC operation: this must use the pruned arm.
    binary = compile_source(compiler, directory, "delivery", '''
let interrupts = 0;
let terminations = 0;
const timer = setInterval(() => {}, 50);
process.on("SIGINT", () => { interrupts++; console.log("interrupt"); });
process.once("SIGTERM", () => { terminations++; console.log("terminate"); clearInterval(timer); });
process.on("exit", () => { console.log("exit", interrupts, terminations); });
console.log("ready");
''')
    assert not has_regex(binary), "signal delivery unexpectedly retains regex"
    with tempfile.TemporaryFile(mode="w+") as stdout, tempfile.TemporaryFile(mode="w+") as stderr:
        child = subprocess.Popen([str(binary)], stdout=stdout, stderr=stderr)
        try:
            def wait_for(marker):
                deadline = time.monotonic() + 5
                while time.monotonic() < deadline:
                    stdout.seek(0)
                    if marker in stdout.read():
                        return
                    if child.poll() is not None:
                        break
                    time.sleep(0.01)
                raise AssertionError("signal child did not report " + marker)
            wait_for("ready")
            child.send_signal(signal.SIGINT)
            wait_for("interrupt")
            child.send_signal(signal.SIGTERM)
            wait_for("terminate")
            assert child.wait(timeout=5) == 0
            stdout.seek(0)
            assert stdout.read() == "ready\ninterrupt\nterminate\nexit 1 1\n"
        finally:
            if child.poll() is None:
                child.kill()
                child.wait()
    binary = compile_source(compiler, directory, "removal", '''
let count = 0;
const listener = () => { count++; };
process.on("SIGINT", listener);
process.removeListener("SIGINT", listener);
console.log(process.listenerCount("SIGINT"));
process.once("SIGTERM", listener);
process.off("SIGTERM", listener);
console.log(process.listenerCount("SIGTERM"), count);
''')
    assert run([str(binary)], timeout=5).stdout == "0\n0 0\n"


def check_ipc(compiler, directory):
    binary = compile_source(compiler, directory, "ipc", '''
process.on("message", (message: any) => {
  console.log("regex", message.pattern.test("abc"));
  process.send(message);
  process.disconnect();
});
console.log("ready");
''')
    assert has_regex(binary), "IPC must retain regex deserialization"
    driver = directory / "ipc-driver.cjs"
    driver.write_text('''
const { spawn } = require('node:child_process');
const assert = require('node:assert/strict');
const receiverOnly = process.argv[3] === 'receive-only';
const child = spawn(process.argv[2], [], {
  stdio: ['ignore', 'pipe', 'pipe', 'ipc'], serialization: 'advanced'
});
let output = '', errors = '', replied = false, sent = false;
const timeout = setTimeout(() => { child.kill(); throw Error('IPC timeout'); }, 5000);
child.stderr.on('data', chunk => { errors += chunk; });
child.stdout.on('data', chunk => {
  output += chunk;
  if (!sent && output.includes('ready\\n')) {
    sent = true;
    child.send({ pattern: /b/gi, values: [1, 'two', null], nested: { ok: true } });
  }
});
child.on('message', message => {
  assert.ok(message.pattern instanceof RegExp);
  assert.equal(message.pattern.source, 'b');
  assert.equal(message.pattern.flags, 'gi');
  assert.deepEqual(message.values, [1, 'two', null]);
  assert.deepEqual(message.nested, { ok: true });
  replied = true;
});
child.on('close', code => {
  clearTimeout(timeout);
  assert.equal(code, 0, errors);
  assert.equal(replied, !receiverOnly);
  assert.equal(output, 'ready\\nregex true\\n');
  console.log(receiverOnly ? 'IPC listener-only deserialization passed' : 'advanced IPC round trip passed');
});
''')
    print(run(["node", str(driver), str(binary)], timeout=10).stdout.strip())
    receiver = compile_source(compiler, directory, "ipc-receive", '''
process.once("message", (message: any) => {
  console.log("regex", message.pattern.test("abc"));
  process.exit(0);
});
console.log("ready");
''')
    assert has_regex(receiver), "message listeners alone must retain regex deserialization"
    print(run(["node", str(driver), str(receiver), "receive-only"], timeout=10).stdout.strip())


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline")
    args = parser.parse_args()
    compiler = os.environ.get("PERRY_BIN", "perry")
    compiler = shutil.which(compiler) or str(Path(compiler).resolve())
    results = {}
    with tempfile.TemporaryDirectory(prefix="perry-feature-pruning-") as temporary:
        directory = Path(temporary)
        for label, executable in [("baseline", args.baseline), ("candidate", compiler)]:
            if not executable:
                continue
            results[label] = {}
            for name, source in SOURCES.items():
                binary = compile_source(executable, directory, name, source)
                regex = has_regex(binary)
                if label == "candidate":
                    assert regex == (name in ("regex", "unknown")), (name, regex)
                else:
                    assert regex == (name not in ("control", "find")), ("baseline", name, regex)
                stdout = run([str(binary)], timeout=10).stdout
                assert stdout == ("ok\n" if name in ("control", "signal") else "818035329\n" if name == "steady" else "1\n"), (name, stdout)
                results[label][name] = dict(size(binary), regex=regex)
                results[label][name]["median_ms"] = timed_runs(binary, 9 if name == "steady" else 15)
        check_signals(compiler, directory)
        check_ipc(compiler, directory)
    print(json.dumps(results, indent=2))
    print("feature pruning regressions passed")


if __name__ == "__main__":
    main()
