#!/usr/bin/env python3
"""Capture the golden fixtures that pin `scripts/parity_normalize.py`.

This is a provenance record, not something CI runs. The `.expected` files are
what the unit test asserts, and each of them was produced by the implementation
this module replaced — so the pair is the proof that the refactor changed no
verdict.

Two legacy sources are captured:

  * the shell chain in `run_parity_tests.sh` (`normalize_output`,
    `normalize_failure_output`) — sources the `output__*.in` and
    `failure__*.in` fixtures against the `parity` profile;
  * `normalize()` in `scripts/node_suite_run.py` — sources the
    `node_suite__*.in` fixtures against the `node_suite` profile.

Neither implementation exists in the tree any more, so pointing these two
environment variables at a pre-refactor revision is how the fixtures would be
recaptured:

    LEGACY_RUNNER=<rev>:run_parity_tests.sh \\
    LEGACY_NODE_SUITE=<rev>:scripts/node_suite_run.py \\
    python3 tests/fixtures/parity_normalize/_generate.py

With the defaults (the working tree) the script stops with an explicit error
instead of silently writing fixtures from the wrong source.
"""

import os
import re
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, "..", "..", ".."))

DEFAULT_RUNNER = os.path.join(ROOT, "run_parity_tests.sh")
DEFAULT_NODE_SUITE = os.path.join(ROOT, "scripts", "node_suite_run.py")

# --- `parity` profile -------------------------------------------------------
PARITY_CASES = {
    "output__buffer-lines": (
        "<Buffer 48 65 6c 6c 6f>\n"
        "<Buffer ff fe>\n"
        "<Buffer zz 41>\n"
        "<Buffer 41000042>\n"
    ),
    "output__unsettled-await": (
        "before\n"
        "Warning: Detected unsettled top-level await at file:///x/test.ts:3:1\n"
        "    at async file:///x/test.ts:3:1\n"
        "after-unsettled\n"
        "after\n"
    ),
    "output__trailing-whitespace": (
        "padded   \n"
        "tabs\t\t\n"
        "clean\n"
        "   leading kept\n"
    ),
    "output__boolean-lines": (
        "true\n"
        "false\n"
        "not true\n"
        "true-ish\n"
        " true\n"
    ),
    "output__float-precision": (
        "0.30000000000000004\n"
        "1.23456789012345678\n"
        "3.14159\n"
        "nested 2.718281828459045 4.669201609102990671\n"
    ),
    "output__console-time": (
        "timerA: 0.123ms\n"
        "timerB: 1.5 s\n"
        "timerC: 42μs\n"
        "timerLog: 2.5ms extra payload\n"
        "timerD: 0.1 ms\n"
        "notATimer: fast\n"
    ),
    "output__node-test-durations": (
        "▶ suite\n"
        "  ✔ case one (1.234ms)\n"
        "  ✖ case two (12ms) trailing\n"
        "  ﹣ skipped (0.5ms)\n"
        "ℹ duration_ms 42.4242\n"
        "ℹ duration_ms 7\n"
    ),
    "output__console-trace-frames": (
        "Trace: here\n"
        "    at Object.<anonymous> (/x/test.ts:1:1)\n"
        "    at Module._compile (node:internal/modules/cjs/loader:1:2)\n"
        "    1: js_closure_call\n"
        "             at /x/test.ts:2:3\n"
        "        (… 4 more identical frames)\n"
        "  (… 9 more identical frames)\n"
        "kept  at mid-line\n"
    ),
    "output__node-warnings": (
        "(node:1234) [MODULE_TYPELESS_PACKAGE_JSON] Warning: something\n"
        "(node:1234) DeprecationWarning: old api\n"
        "(node:1234) [DEP0005] DeprecationWarning: Buffer() is deprecated\n"
        "(node:1234) ExperimentalWarning: Type Stripping is an experimental feature\n"
        "(node:1234) ExperimentalWarning: glob is an experimental feature\n"
        "(node:1234) ExperimentalWarning: WASI is an experimental feature\n"
        "(node:1234) TimeoutOverflowWarning: 2000 does not fit\n"
        "Timeout duration was set to 1.\n"
        "(Use `node --trace-deprecation ...` to show where the warning was created)\n"
        "Reparsing as ES module because module syntax was detected\n"
        'To eliminate this warning, add "type": "module" to /x/package.json.\n'
        "(Use `node --trace-warnings ...` to show where the warning was created)\n"
        "Debugger listening on ws://127.0.0.1:9229/abc\n"
        "For help, see: https://nodejs.org/en/docs/inspector\n"
        "kept warning line\n"
    ),
    "output__pid-prefix": (
        "(node:1) first\n"
        "(node:999999) second\n"
        "(node:12)DeprecationWarning-kept-shape\n"
    ),
    "output__instrument-lines": (
        "[gc-schedule] seed=1234\n"
        "real output\n"
        "[object-dictionary] rows=3\n"
        "[gc-sweep-quarantine] on: old\n"
        "[gc-sweep-quarantine] off: kept\n"
    ),
    "output__console-warning-lines": (
        "(node:5) Warning: Count for label 'x' does not exist\n"
        "Warning: No such label 'y' for console.timeEnd()\n"
        "(node:5) Warning: Label 'z' already exists for console.time()\n"
        "kept\n"
    ),
    # The historical chain trimmed trailing whitespace BEFORE its `$`-anchored
    # console-warning delete, but AFTER the prefix-anchored deletions. So
    # padding on a warning line HIDES it from the delete, while padding on a
    # `Debugger listening …` / `For help, see: …` line does NOT make those
    # survive — they were evaluated before the trim. This pair pins that
    # ordering, which is observable and was wrong in the first extraction.
    "output__warning-after-trim": (
        "Warning: Count for label 'x' does not exist   \n"
        "(node:5) Warning: Label 'y' already exists for console.time()\t\n"
        "Debugger listening on ws://127.0.0.1:9229/abc   \n"
        "For help, see: https://nodejs.org/en/docs/inspector\t \n"
        "kept   \n"
    ),
    "output__trailing-empty-lines": (
        "line one\n"
        "line two\n"
        "\n"
        "\n"
        "\n"
    ),
    "output__carriage-returns": (
        "one\r\n"
        "two\rthree\n"
    ),
    "output__tracepromise-warning": (
        "(node:7) Warning: tracePromise was called with the function fn returned a non-thenable.\n"
        "kept\n"
    ),
    "output__empty": "",
    "failure__node-test-failing-tail": (
        "✔ passing thing\n"
        "✖ failing tests:\n"
        "test at /x/a.test.ts:1:1\n"
        "  Error: boom\n"
    ),
    "failure__error-signature": (
        "line one\n"
        "line two\n"
        "file:///x/test.ts:3:9\n"
        "TypeError: boom happened\n"
        "    at foo (/x/test.ts:3:9)\n"
    ),
    "failure__error-with-node-marker": (
        "prefix line\n"
        "node:internal/modules/run_main:111\n"
        "Error [ERR_MISSING]: no\n"
        "details after\n"
    ),
    "failure__iterable-signature": (
        "Error: somewhere\n"
        "TypeError: xs is not iterable\n"
    ),
    "failure__received-function": (
        "Error: mismatch\n"
        "Received function myHandler\n"
    ),
    "failure__no-error": (
        "just output\n"
        "no error at all\n"
    ),
}

# --- `node_suite` profile ---------------------------------------------------
NODE_SUITE_CASES = {
    "node_suite__durations": (
        "timerA: 0.123ms\n"
        "timerB: 1.5s\n"
        "timerC: 12ms\n"
        "kept: fast\n"
        "12ms without anchor\n"
    ),
    "node_suite__clock-format": (
        "1:23.456 (m:ss.mmm)\n"
        "0:00.012 (h:mm:ss.mmm)\n"
    ),
    "node_suite__frames": (
        "    at Object.<anonymous> (/x/test.ts:1:1)\n"
        "      3: native frame\n"
        "        (… 4 more identical frames)\n"
        "  at two-space-indent\n"
        "  … 9 more identical frames keep\n"
    ),
    "node_suite__test-reporter": (
        "▶ suite\n"
        "    test at /x/a.test.ts:1:1\n"
        "  ✔ case one (1.234ms)\n"
        "  ✖ case two (2s)\n"
        "ℹ duration_ms 42.4242\n"
        "plain output\n"
    ),
    # A differential run against the legacy implementation surfaced that this
    # profile never pins carriage returns. It preserves them (the parity chain
    # is the one that drops them via its `tr -d '\r'` stage), so a fixture must
    # say so — the two profiles differ here on purpose.
    "node_suite__carriage-returns": (
        "one\r\n"
        "two\rthree"
    ),
    # And this profile does NOT decode `<Buffer …>` dumps: that is a parity-chain
    # rule. Pinning it keeps the difference documented rather than assumed.
    "node_suite__buffer-not-decoded": (
        "<Buffer 48 65 6c 6c 6f>\n"
        "kept\n"
    ),
}


def _require(path, marker, env_name):
    if os.path.isfile(path) and marker in open(path, encoding="utf-8").read():
        return
    rel = os.path.relpath(path, ROOT)
    sys.stderr.write(
        f"ERROR: {rel} does not contain {marker}.\n"
        f"Point {env_name} at a pre-refactor revision, for example\n"
        f"  {env_name}=<rev>:{rel} {os.path.basename(__file__)}\n"
    )
    raise SystemExit(2)


def _extract_shell(runner, fn_name):
    rows = open(runner, encoding="utf-8").read().split("\n")
    try:
        start = rows.index(fn_name + "() {")
    except ValueError:
        sys.stderr.write(
            "ERROR: %s has no %s(). Point LEGACY_RUNNER at a pre-refactor revision.\n"
            % (runner, fn_name)
        )
        raise SystemExit(2) from None
    end = rows.index("}", start)
    return "\n".join(rows[start:end + 1])


def _capture_parity(runner):
    functions = "\n".join([
        "PYTHON_CMD=python3",
        _extract_shell(runner, "normalize_output"),
        _extract_shell(runner, "normalize_failure_output"),
    ])
    for name, text in sorted(PARITY_CASES.items()):
        profile, _ = name.split("__", 1)
        fn = "normalize_output" if profile == "output" else "normalize_failure_output"
        proc = subprocess.run(
            ["bash", "-c", functions + '\n%s "$1"' % fn, "gen", text],
            capture_output=True, text=True, check=True,
        )
        _write(name, text, proc.stdout)


def _extract_node_suite_normalize(path):
    """Pull the regex constants and `normalize()` out by source.

    `scripts/node_suite_run.py` is a script: importing it runs its whole sweep,
    so the module cannot be imported to borrow one function. The extracted span
    is exactly the environment-variant scrubber, from its first compiled pattern
    to the line after `normalize()` ends.
    """
    rows = open(path, encoding="utf-8").read().split("\n")
    start = next((i for i, row in enumerate(rows) if row.startswith("_DUR_MS = ")), None)
    end = next((i for i, row in enumerate(rows) if row.startswith("FIXTURE_DIR = ")), None)
    if start is None or end is None or end <= start:
        sys.stderr.write(
            "ERROR: %s does not carry the regex block plus normalize(). "
            "Point LEGACY_NODE_SUITE at a pre-refactor revision.\n" % path
        )
        raise SystemExit(2)
    return "\n".join(rows[start:end])


def _capture_node_suite(path):
    namespace = {"re": re}
    exec(compile(_extract_node_suite_normalize(path), path, "exec"), namespace)
    normalize = namespace["normalize"]
    for name, text in sorted(NODE_SUITE_CASES.items()):
        _write(name, text, normalize(text))


def _write(name, text, expected):
    open(os.path.join(HERE, name + ".in"), "w", encoding="utf-8", newline="").write(text)
    open(os.path.join(HERE, name + ".expected"), "w", encoding="utf-8", newline="").write(expected)
    print("captured %-38s %s" % (name, repr(expected)[:64]))


def main():
    runner = os.environ.get("LEGACY_RUNNER", DEFAULT_RUNNER)
    node_suite = os.environ.get("LEGACY_NODE_SUITE", DEFAULT_NODE_SUITE)
    _require(runner, "normalize_output() {", "LEGACY_RUNNER")
    _require(node_suite, "def normalize(", "LEGACY_NODE_SUITE")
    _capture_parity(runner)
    _capture_node_suite(node_suite)
    return 0


if __name__ == "__main__":
    sys.exit(main())
