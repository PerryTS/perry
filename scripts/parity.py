#!/usr/bin/env python3
"""Run Perry's program cases against Node and gate them on per-subject snapshots.

    parity.py run [SELECTOR ...] [--suite test-files|node-suite|all] [--filter S]
                  [--shard N/M] [--jobs N] [--mode fast|auto] [--update]
                  [--report PATH] [--unpinned-oracle]
    parity.py accept REPORT [REPORT ...] [--mode fast|auto] [--platform P]

Runnable formats are .ts, .cts, .mts and test_*.cjs in every root. Keep the
extension to preserve Node's module mode; case ids omit TS-family suffixes
but retain .cjs. Subject fixtures are excluded from discovery.

A SELECTOR is a subject (`test-files`, or `<root>/<subject>` under test-parity/
for a root in SUBJECT_ROOTS), a whole root, or a path to a case file or a
directory of cases. Every case is compiled, run, and
compared with Node on merged stdout+stderr and exit status after the `unified`
profile of scripts/parity_normalize.py.

A subject's snapshot, test-parity/snapshots/<platform>-<mode>/<subject>.json,
lists its cases that do not pass; an unlisted case must pass. The check runs
both ways over the cases this run executed. A crash is never a snapshot entry.

Exit: 0 when the run matches its snapshots, 1 on divergence, 2 on harness error
(including a selection that ran no case).
"""

from __future__ import annotations

import argparse
import json
import os
import platform as host
import re
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import time
from collections import defaultdict
from concurrent.futures import ThreadPoolExecutor, as_completed
from datetime import date, datetime, timezone
from pathlib import Path

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from parity_normalize import normalize_parity_failure, normalize_unified_output  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent
SCRIPTS = Path(__file__).resolve().parent
WINDOWS = os.name == "nt"
EXE = ".exe" if WINDOWS else ""

STATUSES = ("pass", "parity_fail", "compile_fail", "crash", "node_fail", "skipped")
TRIAGE_FIELDS = ("issue", "added", "category", "reason")
# One 30s run budget for every case: cases share the machine with parallel
# compiles, and a slow-but-correct case must not read as a crash.
RUN_TIMEOUT = 30
COMPILE_TIMEOUT = 300
TOOLCHAIN_TIMEOUT = 900
MAX_OUTPUT_LINES = 50000

SKIP_TESTS = {
    "test_timer", "test_date", "test_math", "test_require", "test_enum",
    "test_decorators", "test_crypto", "test_fs", "test_path", "test_integration_app",
    "test_tls_connect", "test_issue58_object_string", "test_issue63_arr",
    "test_issue63_escape",
}
# These two pin the no-auto HTTP pump link even when the mode is auto.
FORCE_NO_AUTO = {"test_gap_11174_http_runtime_defaults", "test_gap_11301_namespace_member_values"}
EXT_ROUTED = re.compile(
    r"""(from|import|require\()\s*\(?["'](node:)?(http|https|http2|net|ws|zlib|events)["']""")
WASM = re.compile(r"WebAssembly|\.wasm\b")
# Port binding, process spawning and timer ordering are corrupted by sharing
# the machine, so these cases run one at a time after the parallel pool.
SERIAL_MODULES = (
    "http", "http2", "https", "net", "dgram", "tls", "cluster", "dns", "async_hooks",
    "stream", "child_process", "worker_threads", "inspector", "inspector-promises",
    "repl", "diagnostics_channel", "timers", "perf_hooks", "fetch", "v8", "trace_events",
)
SERIAL_IMPORT = re.compile(
    r"""(from|import|require\()\s*\(?["'](node:)?(%s)(/[\w/]+)?["']""" % "|".join(SERIAL_MODULES))


def runs_serially(cid: str, text: str, mode: str) -> bool:
    if cid.startswith("node-suite/"):
        return subject_of(cid).split("/")[-1] in SERIAL_MODULES or bool(SERIAL_IMPORT.search(text))
    # Fast mode builds an ext-routed case's runtime with auto-optimize; one at a
    # time, so concurrent cases never rebuild the same feature set together.
    return bool(SERIAL_IMPORT.search(text)) or (mode == "fast" and bool(EXT_ROUTED.search(text)))


def compile_budget(name: str, text: str, auto: bool, ext_routed: bool) -> int:
    # A compile that may build native toolchain artifacts (an auto-optimize
    # runtime rebuild, ext wrappers, a wasm host) takes minutes on hosted runners.
    if auto or (name in FORCE_NO_AUTO and ext_routed) or WASM.search(text):
        return TOOLCHAIN_TIMEOUT
    return COMPILE_TIMEOUT


# node-suite modules whose no-auto links need prebuilt wrapper archives built in
# the same cargo invocation as the stdlib archive.
EXT_ARCHIVES = {
    "http": ("perry_ext_http", "perry_ext_net", "perry_ext_ws"),
    "https": ("perry_ext_http", "perry_ext_net", "perry_ext_ws"),
    "http2": ("perry_ext_http", "perry_ext_net", "perry_ext_ws"),
    "net": ("perry_ext_net",),
}
EXT_BUILD = (
    "cargo build --release -p perry -p perry-runtime -p perry-stdlib -p perry-runtime-static "
    "-p perry-stdlib-static -p perry-ext-http -p perry-ext-net -p perry-ext-ws "
    "--features perry-stdlib/external-http-server-pump,perry-stdlib/external-http-client-pump"
)
# A case that names a port gets the server listening on it, started once per run.
COMPANIONS = {
    17891: ("test_net_echo_server.py", []),
    17892: ("test_net_upgrade_tls_server.py", ["--port", "17892"]),
}


class HarnessError(Exception):
    pass


# --- selection ---------------------------------------------------------------

# Directories under test-parity/ whose second path level is a subject.
SUBJECT_ROOTS = ("node-suite", "language", "typescript", "perry", "npm")


def suite_dirs(root: Path) -> tuple[Path, Path]:
    return root / "test-files", root / "test-parity" / "node-suite"


def subject_root(root: Path, path: Path) -> Path | None:
    return next((d for name in SUBJECT_ROOTS
                 if (d := root / "test-parity" / name) == path or d in path.parents), None)


def is_case(root: Path, path: Path) -> bool:
    runnable = path.suffix in (".ts", ".cts", ".mts") or (
        path.suffix == ".cjs" and path.name.startswith("test_"))
    if not runnable:
        return False
    if path.parent == suite_dirs(root)[0]:
        return True
    base = subject_root(root, path)
    return base is not None and "fixtures" not in path.relative_to(base).parts


def case_id(root: Path, path: Path) -> str:
    base = subject_root(root, path)
    name = f"{base.name}/" + path.relative_to(base).as_posix() if base is not None else path.name
    return re.sub(r"\.(m|c)?ts$", "", name)


def subject_of(cid: str) -> str:
    parts = cid.split("/")
    return "/".join(parts[:2]) if parts[0] in SUBJECT_ROOTS else "test-files"


def discover(root: Path, target: Path) -> list[Path]:
    files = suite_dirs(root)[0]
    candidates = target.iterdir() if target == files else target.rglob("*")
    return [p for p in candidates if p.is_file() and is_case(root, p)]


def resolve(root: Path, selector: str) -> Path:
    if selector == "test-files":
        return suite_dirs(root)[0]
    if selector.split("/")[0] in SUBJECT_ROOTS:
        return root / "test-parity" / selector
    return Path(selector).resolve()


def select(root: Path, selectors=(), suite="all", filters=(), shard=None) -> list[tuple[str, Path]]:
    root = root.resolve()
    files = suite_dirs(root)[0]
    if not selectors:
        selectors = [suite] if suite != "all" else [
            s for s in ("test-files", *SUBJECT_ROOTS)
            if s == "test-files" or (root / "test-parity" / s).is_dir()]
    paths: set[Path] = set()
    for selector in selectors:
        target = resolve(root, selector)
        if target.is_dir() and (target == files or subject_root(root, target) is not None):
            paths.update(discover(root, target))
        elif target.is_file() and is_case(root, target):
            paths.add(target)
        else:
            raise HarnessError(f"selector {selector!r} names no case or subject")
    picked = []
    for path in sorted(paths):
        cid = case_id(root, path)
        name = re.sub(r"\.(m|c)?ts$", "", path.name)
        if all(f in name or f in cid for f in filters):
            picked.append((cid, path))
    if shard:
        index, total = shard
        picked = picked[index - 1::total]
    return picked


# --- one case ----------------------------------------------------------------

def parse_headers(text: str) -> dict:
    def first(key):
        match = re.search(r"^[ \t]*//[ \t]*parity-%s:[ \t]*(.*)$" % key, text, re.M)
        return match.group(1).strip() if match else None

    env = dict(token.split("=", 1) for token in (first("env") or "").split())
    return {
        "argv": (first("argv") or "").split(),
        "node_argv": (first("node-argv") or "").split(),
        "env": env,
        "skip": first("skip"),
        "rlimit": first("rlimit-as-mb") is not None,
    }


def expected_for(root: Path, cid: str) -> tuple[Path | None, int]:
    """Expected output and exit, looked up by case id, then by basename."""
    def lookup(kind):
        keys = dict.fromkeys((cid, cid.rsplit("/", 1)[-1]))
        return next((p for k in keys if (p := root / "test-parity" / kind / f"{k}.txt").is_file()), None)

    output, code = lookup("expected"), lookup("expected-exit")
    if output is None:
        return None, 0
    return output, int(code.read_text().strip()) if code else 0


def run(cmd, timeout, cwd, env, out_path) -> int:
    """Exit status in shell form: 128+signal for a signal death, 124 on timeout."""
    with open(out_path, "wb") as out:
        kwargs = ({"creationflags": subprocess.CREATE_NEW_PROCESS_GROUP} if WINDOWS
                  else {"start_new_session": True})
        proc = subprocess.Popen(cmd, cwd=cwd, env=env, stdout=out, stderr=subprocess.STDOUT,
                                stdin=subprocess.DEVNULL, **kwargs)
        try:
            code = proc.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            code = 124
        # Reaps the whole group: forked workers outlive their parent otherwise.
        if WINDOWS:
            subprocess.run(["taskkill.exe", "/PID", str(proc.pid), "/T", "/F"],
                           capture_output=True, check=False)
        else:
            try:
                os.killpg(proc.pid, signal.SIGKILL)
            except (ProcessLookupError, PermissionError):
                pass
        proc.wait()
    return 128 - code if code < 0 else code


def read_capped(path: Path) -> str:
    lines = path.read_bytes().decode("utf-8", "replace").replace("\x00", "").split("\n")
    if lines and lines[-1] == "":
        lines.pop()
    if len(lines) > MAX_OUTPUT_LINES:
        lines = lines[:MAX_OUTPUT_LINES] + [f"TRUNCATED at {MAX_OUTPUT_LINES} lines (total: {len(lines)})"]
    return "\n".join(lines)


FORK_CAP_FAILED = "parity: the per-test fork cap could not be applied"


def abnormal(code: int) -> bool:
    return code == 124 or code > 128


def process_budget(jobs: int = 1) -> list[str]:
    """A per-test fork cap at the user's current process count + 50 per worker."""
    if WINDOWS:
        return []
    fmt = ["-L", "-o", "lwp="] if sys.platform.startswith("linux") else ["-o", "pid="]
    listed = subprocess.run(["ps", "-u", str(os.getuid()), *fmt], capture_output=True, text=True)
    count = len([line for line in listed.stdout.splitlines() if line.strip()])
    if listed.returncode != 0 or count <= 0:
        raise HarnessError("cannot measure the current user process count")
    # bash, not sh: dash (Ubuntu's /bin/sh) has no `ulimit -u`.
    # The count includes every concurrent case, so headroom scales with workers.
    return ["bash", "-c", f'ulimit -u {count + 50 * max(1, jobs)} || '
            f'{{ echo "{FORK_CAP_FAILED}" >&2; exit 125; }}; exec "$@"', "_"]


def run_case(ctx: dict, cid: str, path: Path) -> tuple[str, str]:
    root, mode = ctx["root"], ctx["mode"]
    text = path.read_text(encoding="utf-8", errors="replace")
    name = re.sub(r"\.(m|c)?ts$", "", path.name)
    hdr = parse_headers(text)
    if hdr["skip"]:
        return "skipped", hdr["skip"]
    if name in SKIP_TESTS:
        return "skipped", "suite skip list"
    limits = [sys.executable, "-B", str(SCRIPTS / "parity_fixture_limits.py")]
    if hdr["rlimit"] and subprocess.run([*limits, "--print-limit", str(path)],
                                        capture_output=True).returncode != 0:
        raise HarnessError(f"{cid}: invalid parity-rlimit-as-mb header")

    work = Path(tempfile.mkdtemp(prefix="perry-parity-"))
    try:
        base = dict(os.environ)
        node_env = {**base, "FORCE_COLOR": "0", "NO_COLOR": "1", "NODE_DISABLE_COLORS": "1"}
        source, compile_cmd = path, ["compile"] if path.suffix == ".cjs" else []
        node_cmd = [ctx["node"], "--experimental-strip-types", *hdr["node_argv"]]
        node_exit = run([*node_cmd, str(source), *hdr["argv"]], RUN_TIMEOUT, root, node_env, work / "node")
        # A failed import-free globals script retries as CommonJS: the repo's
        # `"type": "module"` makes a plain `.ts` strict-mode ESM for both runtimes.
        if (node_exit != 0 and cid.startswith("node-suite/globals/") and path.suffix == ".ts"
                and not re.search(r"^\s*(import|export)[\s{]", text, re.M)):
            source, compile_cmd = work / f"{name}.cts", ["compile"]
            shutil.copy(path, source)
            node_exit = run([*node_cmd, str(source), *hdr["argv"]], RUN_TIMEOUT, root, node_env,
                            work / "node")
        if abnormal(node_exit):
            return "node_fail", f"Node exit {node_exit}"
        node_out = read_capped(work / "node")

        env = {**base, **hdr["env"]}
        env.pop("PERRY_NO_AUTO_OPTIMIZE", None)
        if name.startswith("test_parity_") or cid.startswith("node-suite/"):
            env["PERRY_ALLOW_UNIMPLEMENTED"] = "1"
        ext_routed = bool(EXT_ROUTED.search(text))
        # A test-files case routed to an ext wrapper cannot be served by one
        # prebuilt stdlib, so fast mode lets auto-optimize build a coherent set.
        auto = name not in FORCE_NO_AUTO and (
            mode == "auto" or (ext_routed and not cid.startswith("node-suite/")))
        if not auto:
            env["PERRY_NO_AUTO_OPTIMIZE"] = "1"
        flags = []
        if re.match(r"node-suite/[^/]+/.*(webassembly|wasm)", cid):
            flags.append("--enable-wasm-runtime")
        budget = compile_budget(name, text, auto, ext_routed)
        binary = work / f"case{EXE}"
        compile_line = [ctx["perry"], *compile_cmd, *flags, str(source), "-o", str(binary)]
        code = run(compile_line, budget, root, env, work / "compile")
        if code != 0 and "perry-jsruntime" in read_capped(work / "compile"):
            compile_line.insert(-3, "--enable-js-runtime")
            code = run(compile_line, budget, root, env, work / "compile")
        if code != 0:
            tail = "".join(f"\n       {line}" for line in read_capped(work / "compile").splitlines()[-3:])
            # A compiler that dies or hangs is a crash, which no snapshot accepts.
            if abnormal(code):
                return "crash", f"compiler exit {code}" + tail
            return "compile_fail", "compile error" + tail

        run_env = {**base, "PERRY_STUB_DIAG": "off", **hdr["env"]}
        prefix = process_budget(ctx["jobs"]) + ([*limits, str(path)] if hdr["rlimit"] else [])
        perry_exit = run([*prefix, str(binary), *hdr["argv"]], RUN_TIMEOUT, root, run_env,
                         work / "perry")
        perry_out = read_capped(work / "perry")
        if perry_exit == 125 and FORK_CAP_FAILED in perry_out:
            return "crash", FORK_CAP_FAILED
        if abnormal(perry_exit):
            last = perry_out.splitlines()[-1:] or [""]
            return "crash", f"Perry exit {perry_exit}; last line: {last[0]}"

        expected, expected_exit = expected_for(root, cid)
        if expected:
            want, want_exit = normalize_unified_output(expected.read_text(errors="replace")), expected_exit
            got = normalize_unified_output(perry_out)
        else:
            want, got, want_exit = (normalize_unified_output(node_out),
                                    normalize_unified_output(perry_out), node_exit)
            if node_exit != 0:
                want, got = normalize_parity_failure(want), normalize_parity_failure(got)
        if perry_exit == want_exit and want == got:
            return "pass", ""
        first = next((f"want {a!r}, got {b!r}" for a, b in zip(want.split("\n"), got.split("\n"))
                      if a != b), f"want {len(want.splitlines())} lines, got {len(got.splitlines())}")
        return "parity_fail", f"exit want {want_exit} got {perry_exit}; {first}"
    finally:
        shutil.rmtree(work, ignore_errors=True)


# --- snapshots ---------------------------------------------------------------

def case_exists(root: Path, cid: str) -> bool:
    files = suite_dirs(root)[0]
    if cid.split("/")[0] in SUBJECT_ROOTS:
        return (root / "test-parity" / f"{cid}.ts").is_file()
    return any((files / f"{cid}{ext}").is_file() for ext in ("", ".ts", ".cts", ".mts"))


def snapshot_path(root: Path, plat: str, mode: str, subject: str) -> Path:
    return root / "test-parity" / "snapshots" / f"{plat}-{mode}" / f"{subject}.json"


def load_snapshot(path: Path) -> dict:
    if not path.is_file():
        return {}
    tests = json.loads(path.read_text(encoding="utf-8")).get("tests", {})
    parked = sorted(cid for cid, entry in tests.items() if entry.get("status") == "crash")
    if parked:
        raise HarnessError(f"{path} accepts a crash, which a snapshot never may: {', '.join(parked)}")
    return tests


def write_snapshot(path: Path, tests: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    payload = {
        "_schema": ("Generated by scripts/parity.py; do not hand-edit statuses. Lists every case "
                    "of this subject that does not pass; an absent case must pass. Triage fields: "
                    + ", ".join(TRIAGE_FIELDS) + "."),
        "tests": dict(sorted(tests.items())),
    }
    path.write_text(json.dumps(payload, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")


def diff(results: dict, entries: dict) -> list[tuple[str, str, str, str]]:
    out = []
    for cid, actual in sorted(results.items()):
        expected = entries.get(cid, {}).get("status", "pass")
        if expected != actual:
            kind = ("regression" if expected == "pass"
                    else "improvement" if actual == "pass" else "changed")
            out.append((kind, cid, expected, actual))
    return out


def fold(results: dict, entries: dict, today: str, exists) -> tuple[dict, list[str]]:
    """Fold results in; a crash is returned instead of accepted."""
    merged, crashes = dict(entries), []
    for cid, status in sorted(results.items()):
        if status == "crash":
            crashes.append(cid)
        elif status == "pass":
            merged.pop(cid, None)
        else:
            previous = merged.get(cid, {})
            merged[cid] = {"status": status, "issue": previous.get("issue"),
                           "added": previous.get("added") or today,
                           "category": previous.get("category") or "untriaged",
                           "reason": previous.get("reason") or ""}
    return {cid: e for cid, e in merged.items() if exists(cid)}, crashes


def gate(root, results, plat, mode, update) -> int:
    by_subject = defaultdict(dict)
    for cid, status in results.items():
        by_subject[subject_of(cid)][cid] = status
    diverged = False
    for subject, subset in sorted(by_subject.items()):
        path = snapshot_path(root, plat, mode, subject)
        entries = load_snapshot(path)
        if update:
            folded, crashes = fold(subset, entries, date.today().isoformat(),
                                   lambda cid: case_exists(root, cid))
            if folded or path.exists():
                write_snapshot(path, folded)
            for cid in crashes:
                print(f"NOT ACCEPTED: {cid} crashed; a crash is a defect, never a snapshot entry",
                      file=sys.stderr)
            diverged |= bool(crashes)
            continue
        for kind, cid, expected, actual in diff(subset, entries):
            diverged = True
            print(f"{kind.upper()}: {cid}: {expected} -> {actual}", file=sys.stderr)
    return 1 if diverged else 0


def host_platform() -> str:
    plat = os.environ.get("PERRY_HOST_PLATFORM") or {
        "Windows": "windows", "Darwin": "macos", "Linux": "linux"}.get(host.system(), "other")
    if plat not in ("windows", "macos", "linux", "other"):
        raise HarnessError(f"invalid PERRY_HOST_PLATFORM {plat!r}")
    return plat


# --- commands ----------------------------------------------------------------

def check_oracle(root: Path, node: str) -> tuple[str, list[str]]:
    """The running Node version and every way the oracle differs from its pin."""
    pinned = (root / ".node-version").read_text().strip()
    running = subprocess.run([node, "--version"], capture_output=True, text=True).stdout.strip()
    problems = []
    if running.lstrip("v") != pinned:
        problems.append(f"running Node {running or 'unknown'}, .node-version pins v{pinned}")
    if (root / "package.json").is_file() and subprocess.run(
            ["npm", "ls", "--depth=0", "--silent"], cwd=root, capture_output=True,
            shell=WINDOWS).returncode != 0:
        problems.append("root npm dependencies are missing or stale, so package imports fail in "
                        "Node (run: npm ci --ignore-scripts --no-audit --no-fund)")
    return running, problems


def check_toolchain(root: Path, mode: str, cases) -> tuple[str, Path]:
    perry = Path(os.environ.get("PERRY_BIN") or root / "target" / "release" / f"perry{EXE}")
    if not os.access(perry, os.X_OK):
        raise HarnessError(f"PERRY_BIN is not executable: {perry}. Build it with: "
                           "cargo build --release -p perry -p perry-runtime-static -p perry-stdlib-static")
    runtime = Path(os.environ.get("PERRY_RUNTIME_DIR") or perry.parent)
    os.environ["PERRY_RUNTIME_DIR"] = str(runtime)
    if mode == "fast":
        lib = (lambda stem: f"{stem}.lib") if WINDOWS else (lambda stem: f"lib{stem}.a")
        needed = {"perry_runtime", "perry_stdlib"}
        for cid, path in cases:
            if cid.startswith("node-suite/"):
                needed.update(EXT_ARCHIVES.get(subject_of(cid).split("/")[-1], ()))
            # Named no-auto cases keep their preflight after their IDs gain a subject prefix.
            if cid in FORCE_NO_AUTO or Path(path).stem in FORCE_NO_AUTO:
                needed.update(EXT_ARCHIVES["http"])
        missing = sorted(lib(s) for s in needed if not (runtime / lib(s)).is_file())
        sidecar = runtime / (lib("perry_runtime") + ".gc-effects")
        if not missing and not sidecar.is_file():
            raise HarnessError(f"{sidecar} is missing; perry refuses every link without it. "
                               f"Run: scripts/gc_call_effects/sidecars.sh {runtime}")
        if missing:
            raise HarnessError(f"missing from {runtime}: {', '.join(missing)}. Build every archive "
                               f"in ONE cargo invocation (two give two tokio copies): {EXT_BUILD}")
    return str(perry), runtime


def start_companions(root: Path, cases) -> list[subprocess.Popen]:
    texts = [path.read_text(encoding="utf-8", errors="replace") for _, path in cases]
    procs = []
    for port, (script, args) in COMPANIONS.items():
        if not any(re.search(r"\b%d\b" % port, text) for text in texts):
            continue
        procs.append(subprocess.Popen([sys.executable, str(root / "test-files" / script), *args],
                                      cwd=root))
        for _ in range(50):
            try:
                socket.create_connection(("127.0.0.1", port), timeout=0.2).close()
                break
            except OSError:
                time.sleep(0.1)
    return procs


def write_report(path: Path, plat: str, mode: str, node: str, override: bool, results: dict) -> None:
    count = lambda status: sum(1 for s in results.values() if s == status)  # noqa: E731
    total = count("pass") + count("parity_fail") + count("crash")
    failures = lambda status: sorted(c for c, s in results.items() if s == status)  # noqa: E731
    report = {
        "generated_at": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "platform": plat, "mode": mode, "node": node, "oracle_override": override,
        "summary": {
            "parity_pass": count("pass"), "parity_fail": count("parity_fail"),
            "compile_fail": count("compile_fail"), "crash_fail": count("crash"),
            "node_fail": count("node_fail"), "skipped": count("skipped"), "total_run": total,
            "parity_percentage": round(100 * count("pass") / total, 1) if total else 0.0,
        },
        "failures": {"parity": failures("parity_fail"), "compile": failures("compile_fail"),
                     "crash": failures("crash")},
        "results": [{"id": c, "status": s} for c, s in sorted(results.items())],
    }
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")


def cmd_run(args, root: Path) -> int:
    shard = None
    if args.shard:
        match = re.fullmatch(r"(\d+)/(\d+)", args.shard)
        if not match or not 1 <= int(match.group(1)) <= int(match.group(2)):
            raise HarnessError(f"invalid --shard {args.shard!r} (want N/M, 1 <= N <= M)")
        shard = (int(match.group(1)), int(match.group(2)))
    cases = select(root, args.selectors, args.suite, args.filter or [], shard)
    if not cases:
        raise HarnessError("the selection matched no case; a gate whose subject never ran cannot pass")
    plat, node = host_platform(), os.environ.get("NODE_BIN", "node")
    version, problems = check_oracle(root, node)
    if problems and not args.unpinned_oracle:
        raise HarnessError("the Node oracle is not the pinned one: " + "; ".join(problems)
                           + ". Output is compared byte for byte; pass --unpinned-oracle to explore.")
    perry, _ = check_toolchain(root, args.mode, cases)
    ctx = {"root": root, "mode": args.mode, "perry": perry, "node": node, "jobs": args.jobs}
    if args.mode == "auto":
        # Concurrent first compiles of one subject would each rebuild the same
        # feature-stripped runtime; one serial compile per subject warms it.
        warm = {}
        for cid, path in cases:
            warm.setdefault(subject_of(cid), path)
        for path in warm.values():
            with tempfile.TemporaryDirectory(prefix="perry-parity-warm-") as td:
                run([perry, str(path), "-o", str(Path(td) / f"o{EXE}")], TOOLCHAIN_TIMEOUT, root,
                    dict(os.environ), Path(td) / "log")
    serial = [c for c in cases if runs_serially(
        c[0], c[1].read_text(encoding="utf-8", errors="replace"), args.mode)]
    parallel = [c for c in cases if c not in serial]
    results, done = {}, 0

    def record(cid, status, detail):
        nonlocal done
        done += 1
        results[cid] = status
        print(f"[{done}/{len(cases)}] {status.upper():12} {cid}" + (f" ({detail})" if detail else ""),
              flush=True)

    companions = start_companions(root, cases)
    try:
        with ThreadPoolExecutor(max_workers=args.jobs) as pool:
            futures = {pool.submit(run_case, ctx, cid, path): cid for cid, path in parallel}
            for future in as_completed(futures):
                record(futures[future], *future.result())
        for cid, path in serial:
            record(cid, *run_case(ctx, cid, path))
    finally:
        for proc in companions:
            proc.kill()
            proc.wait()

    write_report(Path(args.report), plat, args.mode, version, bool(problems), results)
    crashes = sorted(c for c, s in results.items() if s == "crash")
    if crashes:
        print("\nCRASHED (signal death or timeout; never accepted into a snapshot):", file=sys.stderr)
        for cid in crashes:
            print(f"  - {cid}", file=sys.stderr)
    counts = {s: sum(1 for v in results.values() if v == s) for s in STATUSES}
    print("\n" + ", ".join(f"{s}={n}" for s, n in counts.items() if n))
    code = gate(root, results, plat, args.mode, args.update)
    print("snapshot " + ("updated" if args.update else "matches" if code == 0 else "DIVERGES")
          + f" ({plat}-{args.mode}, {len(results)} cases)")
    return code


def cmd_accept(args, root: Path) -> int:
    merged, plats, modes = {}, set(), set()
    for path in args.reports:
        data = json.loads(Path(path).read_text(encoding="utf-8"))
        if data.get("oracle_override"):
            raise HarnessError(f"{path} was produced with --unpinned-oracle; it cannot be a baseline")
        plats.add(args.platform or data.get("platform"))
        modes.add(args.mode or data.get("mode"))
        for entry in data.get("results") or []:
            cid, status = entry.get("id"), entry.get("status")
            if not cid or status not in STATUSES:
                raise HarnessError(f"{path}: malformed result {entry!r}")
            if merged.setdefault(cid, status) != status:
                raise HarnessError(f"{cid} is {merged[cid]} in one report and {status} in {path}")
    if len(plats) != 1 or None in plats or len(modes) != 1 or None in modes:
        raise HarnessError(f"reports must share one platform and mode (got {plats}, {modes}); "
                           "pass --platform/--mode")
    if not merged:
        raise HarnessError("the reports hold no results")
    return gate(root, merged, plats.pop(), modes.pop(), update=True)


def main(argv=None, root: Path = ROOT) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    sub = parser.add_subparsers(dest="command", required=True)
    run_p = sub.add_parser("run")
    run_p.add_argument("selectors", nargs="*")
    run_p.add_argument("--suite", choices=("test-files", "node-suite", "all"), default="all")
    run_p.add_argument("--filter", action="append")
    run_p.add_argument("--shard")
    run_p.add_argument("--jobs", type=int, default=os.cpu_count() or 1)
    run_p.add_argument("--mode", choices=("fast", "auto"), default="fast")
    run_p.add_argument("--update", action="store_true")
    run_p.add_argument("--report", default=str(root / "test-parity" / "reports" / "latest.json"))
    run_p.add_argument("--unpinned-oracle", action="store_true")
    acc_p = sub.add_parser("accept")
    acc_p.add_argument("reports", nargs="+")
    acc_p.add_argument("--mode", choices=("fast", "auto"))
    acc_p.add_argument("--platform", choices=("windows", "macos", "linux", "other"))
    args = parser.parse_args(argv)
    try:
        return cmd_run(args, root) if args.command == "run" else cmd_accept(args, root)
    except (HarnessError, OSError, ValueError) as exc:
        print(f"parity: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
