#!/usr/bin/env python3
"""The one definition of "Perry printed what Node printed".

Two parity runners ask this question today:

  * `run_parity_tests.sh` — the `all` / `node-suite` sweep that the release
    gate and the per-PR gap suite both drive;
  * `scripts/node_suite_run.py` — the per-module table the scheduled
    `node-suite-guard` job scores against `test-parity/node_suite_baseline.json`.

They asked it with two separate implementations, one written in a ~30-stage
`sed` chain and one in Python, and each carried rules the other did not. Two
implementations of one question can disagree, and a disagreement here is not a
cosmetic bug: it decides whether a divergence between Perry and Node is
reported at all, and the two runners gate different things.

This module is that single implementation. It keeps **two named rule profiles**
rather than one merged rule set, because the rule sets are not interchangeable
in either direction:

  * `parity` — the historical shell chain, byte for byte. It decodes
    `<Buffer …>` dumps, folds whole-line `true`/`false` to `1`/`0`, truncates
    float precision, normalizes `console.time` output, and strips Node's
    process warnings and `console.trace` frame shapes.
  * `node_suite` — the historical `node_suite_run.py` rules. It masks the
    `node:test` reporter's own artefacts (`test at <location>`,
    `(N.NNNms)`, `ℹ duration_ms`), which the parity chain never learned and
    which are *not* safe to add there: `(N.NNNms)` matches ordinary output, so
    applying it corpus-wide would hide real divergences behind a "duration"
    that was never a duration.

Merging them is a behaviour change in both lanes and needs measured evidence;
until then the profiles are declared here, side by side, so the two can never
drift apart silently. `tests/test_parity_normalize.py` pins both against
fixtures captured from the implementations this module replaced.

Usage (stdin is the captured output, exactly as the shell caller holds it):

    parity_normalize.py output  [--profile parity|node_suite]
    parity_normalize.py failure

`failure` exists only for the `parity` profile: `node_suite_run.py` compares
raw stdout and never runs a failure-signature pass.
"""

from __future__ import annotations

import re
import sys

# ---------------------------------------------------------------------------
# Profile: parity
# ---------------------------------------------------------------------------

# The runner merges stderr into the compared stream (`> "$f" 2>&1`), so every
# Node process-warning line arrives interleaved with program output. These are
# environment noise: the pid is not stable, the warnings appear after the
# script body on Node and eagerly on Perry, and none of them is program
# output. Each pattern is the `sed -E '/…/d'` it replaces, in the same order —
# order matters, because the `(node:<pid>)` rewrite near the end would make the
# earlier anchored deletions stop matching.
_PARITY_DELETE = (
    re.compile(r"^\[gc-schedule\]"),
    re.compile(r"^\[object-dictionary\]"),
    re.compile(r"^\[gc-sweep-quarantine\] on: "),
    re.compile(r"^\(node:[0-9]+\) \[MODULE_TYPELESS_PACKAGE_JSON\]"),
    re.compile(r"^\(node:[0-9]+\)( \[[^]]+\])? DeprecationWarning:"),
    re.compile(r"^\(node:[0-9]+\) ExperimentalWarning: Type Stripping is an experimental feature"),
    re.compile(r"^\(node:[0-9]+\) ExperimentalWarning: glob is an experimental feature"),
    re.compile(r"^\(node:[0-9]+\) ExperimentalWarning: WASI is an experimental feature"),
    re.compile(
        r"^\(node:[0-9]+\) Warning: tracePromise was called with the function "
        r".* returned a non-thenable\.$"
    ),
    re.compile(r"^Debugger listening on ws://[^ \t\n\r\v\f]+$"),
    re.compile(r"^For help, see: https://nodejs\.org/en/docs/inspector$"),
    re.compile(r"^Timeout duration was set to [0-9]+\.$"),
    re.compile(r"^\(Use `node --trace-deprecation"),
    re.compile(r"^Reparsing as ES module because module syntax was detected"),
    re.compile(r'^To eliminate this warning, add "type": "module"'),
    re.compile(r"^\(Use `node --trace-warnings"),
    # Node reports an out-of-range timer duration as a process warning; Perry
    # writes it eagerly. The pid is unstable, so it is masked but the message
    # is kept — a dropped unit or a wrong bound must still surface.
    re.compile(
        r"^(\(node:[0-9]+\) )?Warning: (Count for .* does not exist"
        r"|No such label .* for console\.(timeLog|timeEnd)\(\)"
        r"|Label .* already exists for console\.time\(\))$"
    ),
    # Stack frames: Node's `at <symbol> (<location>)`, Perry's `N: <symbol>`
    # native frames, and the `(… N more identical frames)` collapse. Paths and
    # line numbers vary by machine, so the frames are dropped whole; the
    # `Trace: <message>` header above them is what both sides must match.
    re.compile(r"^[ \t\r\v\f]+at "),
    re.compile(r"^[ \t\r\v\f]+[0-9]+: "),
    re.compile(r"^[ \t\r\v\f]+[(].*more identical frames[)]"),
)

_PARITY_TIMEOUT_PREFIX = re.compile(
    r"^\(node:[0-9]+\) (Timeout(Overflow|Negative|NaN)Warning:)"
)
_PARITY_PID = re.compile(r"^\(node:[0-9]+\) ")
_FLOAT_TRUNCATE = re.compile(r"([0-9]+\.[0-9]{10})[0-9]+")
_TIMER_WITH_TAIL = re.compile(r"^([^:]*): [0-9]+(\.[0-9]+)?[ \t\r\v\f]*(μs|ms|s)( .*)$")
_TIMER_BARE = re.compile(r"^([^:]*): [0-9]+(\.[0-9]+)?[ \t\r\v\f]*(μs|ms|s)$")
_TEST_DURATION_TAIL = re.compile(
    r"^([ \t\r\v\f]*[✔✖﹣] .*) \([0-9]+(\.[0-9]+)?ms\)( .*)$"
)
_TEST_DURATION_BARE = re.compile(
    r"^([ \t\r\v\f]*[✔✖﹣] .*) \([0-9]+(\.[0-9]+)?ms\)$"
)
_REPORTER_DURATION = re.compile(r"^ℹ duration_ms [0-9]+(\.[0-9]+)?$")
_TRAILING_SPACE = re.compile(r"[ \t\r\v\f]*$")
_ERROR_SIGNATURE = re.compile(r"^[A-Za-z]*Error(?: \[[^]]+\])?:")
_NODE_LOCATION = re.compile(r"^file://.*:\d+(?::\d+)?$")


def _decode_buffers(text: str) -> list[str]:
    """First pass of the shell chain, over `<Buffer XX XX …>` dumps.

    A `Buffer` line is printed as the UTF-8 text its hex bytes spell, so a
    `console.log` of a buffer compares as the string Node wrote rather than as
    Perry's hex spelling of it. Invalid UTF-8 becomes U+FFFD (the behaviour the
    old `xxd -r -p` stage had) and an invalid hex sequence passes through
    unchanged, keeping the divergence visible instead of swallowing it.
    """
    rows = text.split("\n")
    if rows and rows[-1] == "":
        # Mirrors `for raw in sys.stdin`: a trailing newline terminates the
        # last line, it does not produce an extra empty one.
        rows.pop()
    out: list[str] = []
    skip_unsettled = 0
    for raw in rows:
        line = raw.rstrip("\n").rstrip("\r")
        if skip_unsettled:
            skip_unsettled -= 1
            continue
        if line.startswith("Warning: Detected unsettled top-level await at "):
            # The location and the two frames under it are Node-specific.
            out.append("Warning: Detected unsettled top-level await")
            skip_unsettled = 2
            continue
        if line.startswith("<Buffer ") and line.endswith(">"):
            hex_part = line[len("<Buffer "):-1].replace(" ", "")
            try:
                out.append(bytes.fromhex(hex_part).decode("utf-8", errors="replace"))
            except ValueError:
                out.append(line)
        else:
            out.append(line)
    return out


def _strip_trailing_empty(lines: list[str]) -> list[str]:
    """`sed -e :a -e '/^\\n*$/{$d;N;ba' -e '}'` — drop only a trailing run."""
    while lines and lines[-1] == "":
        lines.pop()
    return lines


def normalize_parity_output(text: str) -> str:
    lines = _decode_buffers(text)
    # `echo "$decoded"` re-terminates the stream; `tr -d '\\r'` then removes any
    # remaining carriage return anywhere, not just at end of line. NUL goes in
    # the same step: it cannot survive the historical `$(...)` that carried this
    # text, so a decoded `<Buffer 41 00 42>` used to compare as `AB` on both
    # sides. Reproducing that is what makes this refactor verdict-preserving —
    # the byte was never observable to either runner.
    joined = (
        "\n".join(lines).replace("\r", "").replace("\x00", "") if lines else ""
    )
    rows = joined.split("\n") if joined else []

    out: list[str] = []
    for line in rows:
        if any(pattern.search(line) for pattern in _PARITY_DELETE):
            continue
        line = _PARITY_TIMEOUT_PREFIX.sub(r"(node:<pid>) \1", line)
        line = _TRAILING_SPACE.sub("", line)
        if line == "true":
            line = "1"
        elif line == "false":
            line = "0"
        line = _FLOAT_TRUNCATE.sub(r"\1", line)

        match = _TIMER_WITH_TAIL.match(line)
        if match:
            line = "%s: <timer>%s" % (match.group(1), match.group(4))
        else:
            match = _TIMER_BARE.match(line)
            if match:
                line = "%s: <timer>" % match.group(1)

        match = _TEST_DURATION_TAIL.match(line)
        if match:
            line = "%s (<duration>)%s" % (match.group(1), match.group(3))
        else:
            match = _TEST_DURATION_BARE.match(line)
            if match:
                line = "%s (<duration>)" % match.group(1)
        if _REPORTER_DURATION.match(line):
            line = "ℹ duration_ms <duration>"

        line = _PARITY_PID.sub("(node:<pid>) ", line)
        out.append(line)

    out = _strip_trailing_empty(out)
    return "\n".join(out) + "\n" if out else ""


def normalize_parity_failure(text: str) -> str:
    """Compare the deterministic head of a failing run, not its stack.

    The `node:test` default reporter repeats every failed result with source
    locations and stacks after the aggregate summary; those are runtime-specific
    by design. Below the summary, both runtimes print a location marker and then
    an error signature whose *message* is the observable behaviour — so the
    comparison keeps the prefix and the signature and drops the frames.
    """
    lines = text.splitlines()
    if "✖ failing tests:" in lines:
        lines = lines[: lines.index("✖ failing tests:")]
        _strip_trailing_empty(lines)
    index = next((i for i, line in enumerate(lines) if _ERROR_SIGNATURE.match(line)), None)
    if index is None:
        return "\n".join(lines)

    marker = None
    for i in range(index - 1, -1, -1):
        if lines[i].startswith("node:") or _NODE_LOCATION.match(lines[i]):
            marker = i
            break
    prefix = lines[: marker if marker is not None else index]
    _strip_trailing_empty(prefix)

    signature = lines[index]
    signature = re.sub(
        r"^TypeError: [A-Za-z_$][\w$]* is not iterable$",
        "TypeError: is not iterable",
        signature,
    )
    signature = re.sub(
        r"Received function(?: [A-Za-z_$][\w$]*)?$", "Received function", signature
    ).rstrip()
    return "\n".join([*prefix, signature])


# ---------------------------------------------------------------------------
# Profile: node_suite
# ---------------------------------------------------------------------------

# Console timer output: Node renders a live hrtime delta as a raw float, so the
# value is non-deterministic run to run AND has a variable decimal count. Only
# the number is masked; the unit surviving keeps a dropped `ms` visible.
_NS_DUR_MS = re.compile(r"(?<=: )\d+(?:\.\d+)?(ms|s)\b")
_NS_DUR_CLOCK = re.compile(r"\b\d+:\d{2}\.\d{3} \((h:mm|m):ss\.mmm\)")
_NS_FRAME = re.compile(r"^\s{4,}(at\s|\d+:\s)|… \d+ more identical frames")
_NS_TEST_DURATION = re.compile(r"\(\d+(?:\.\d+)?(?:ms|s)\)")
_NS_TEST_LOCATION = re.compile(r"^(\s*)test at .+:\d+:\d+$")
_NS_DURATION_MS_LINE = "ℹ duration_ms "


def normalize_node_suite_output(text: str) -> str:
    out: list[str] = []
    for line in text.split("\n"):
        if _NS_FRAME.search(line):
            continue
        location = _NS_TEST_LOCATION.match(line)
        if location:
            line = "%stest at <location>" % location.group(1)
        line = _NS_DUR_MS.sub(lambda match: "<dur>" + match.group(1), line)
        line = _NS_DUR_CLOCK.sub(lambda match: "<dur:" + match.group(1) + ">", line)
        line = _NS_TEST_DURATION.sub("(<test-duration>)", line)
        stripped = line.lstrip()
        if stripped.startswith(_NS_DURATION_MS_LINE):
            indent = line[: len(line) - len(stripped)]
            line = indent + _NS_DURATION_MS_LINE + "<test-duration>"
        out.append(line)
    return "\n".join(out)


PROFILES = {
    "parity": normalize_parity_output,
    "node_suite": normalize_node_suite_output,
}


def normalize_output(text: str, profile: str = "parity") -> str:
    try:
        normalizer = PROFILES[profile]
    except KeyError:
        raise ValueError(
            "unknown normalize profile %r (want one of %s)"
            % (profile, ", ".join(sorted(PROFILES)))
        ) from None
    return normalizer(text)


def main(argv: list[str]) -> int:
    profile = "parity"
    positional = []
    i = 0
    while i < len(argv):
        arg = argv[i]
        if arg == "--profile":
            if i + 1 >= len(argv):
                sys.stderr.write("--profile needs a value\n")
                return 2
            profile = argv[i + 1]
            i += 2
            continue
        if arg.startswith("--profile="):
            profile = arg[len("--profile="):]
            i += 1
            continue
        positional.append(arg)
        i += 1

    if len(positional) != 1 or positional[0] not in ("output", "failure"):
        sys.stderr.write("usage: parity_normalize.py output|failure [--profile NAME]\n")
        return 2
    mode = positional[0]

    text = sys.stdin.read()
    if mode == "failure":
        if profile != "parity":
            sys.stderr.write("failure normalization is only defined for the parity profile\n")
            return 2
        sys.stdout.write(normalize_parity_failure(text))
        return 0
    try:
        sys.stdout.write(normalize_output(text, profile))
    except ValueError as exc:
        sys.stderr.write("%s\n" % exc)
        return 2
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
