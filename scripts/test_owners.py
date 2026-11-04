#!/usr/bin/env python3
"""Map a pull request's changed paths to the program cases it should run.

    test_owners.py [--base REF]      print one scripts/parity.py selector per line
    test_owners.py --no-new-gap-vs REF   fail when HEAD adds test-files/test_gap_* over REF
    test_owners.py --self-test

A changed case, or its expected output, selects that case; a changed snapshot
selects its subject; a changed source path selects the subjects OWNERS gives
it. A source path no row owns is cross-cutting and selects nothing here: the
pull-request gap lane already runs for every change, so selection only adds to
it and never runs less than it. Changed paths come from `git diff BASE...HEAD`
with --base, else one per line on stdin (CI pipes scripts/ci_pr_files.py).
See docs/src/testing/test-structure.md.
"""

from __future__ import annotations

import argparse
import fnmatch
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "scripts"))

import parity  # noqa: E402

RT = "crates/perry-runtime/src/"
STD = "crates/perry-stdlib/src/"
TABLE = "crates/perry-codegen/src/lower_call/native_table/"
CG = "crates/perry-codegen/src/"
HIR = "crates/perry-hir/src/lower/expr_call/"

# (fnmatch pattern, node-suite modules). `*` crosses `/`. A path matching
# several rows selects the union.
OWNERS: list[tuple[str, tuple[str, ...]]] = [
    (RT + "async_hooks*", ("async_hooks",)),
    (RT + "async_hook_slots.rs", ("async_hooks",)),
    (RT + "async_context*", ("async_hooks",)),
    (RT + "async_local_storage.rs", ("async_hooks",)),
    (RT + "buffer/*", ("buffer",)),
    (RT + "child_process/*", ("child_process",)),
    (RT + "cluster*.rs", ("cluster",)),
    (RT + "builtins/console.rs", ("console",)),
    (RT + "object/native_module/constants*.rs", ("constants",)),
    (RT + "dgram*", ("dgram",)),
    (RT + "node_submodules/diagnostics*", ("diagnostics_channel",)),
    (RT + "dns*", ("dns",)),
    (RT + "event_target*", ("events",)),
    (RT + "event_emitter_error.rs", ("events",)),
    (RT + "fs/*", ("fs", "fs-promises")),
    (RT + "fs_hook_slots.rs", ("fs", "fs-promises")),
    (RT + "node_submodules/fs_promises*", ("fs-promises",)),
    (RT + "node_http2_constants.rs", ("http2",)),
    (RT + "node_inspector.rs", ("inspector", "inspector-promises")),
    (RT + "module_require*", ("module",)),
    (RT + "module_hooks.rs", ("module",)),
    (RT + "process/node_module*", ("module",)),
    (RT + "net_validate.rs", ("net",)),
    (RT + "os/*", ("os",)),
    (RT + "os.rs", ("os",)),
    (RT + "os_network.rs", ("os",)),
    (RT + "os_priority.rs", ("os",)),
    (RT + "os_hook_slots.rs", ("os",)),
    (RT + "path/*", ("path",)),
    (RT + "path.rs", ("path",)),
    (RT + "perf_*", ("perf_hooks",)),
    (RT + "process/*", ("process",)),
    (RT + "process.rs", ("process",)),
    (RT + "process_startup.rs", ("process",)),
    (RT + "punycode.rs", ("punycode",)),
    (RT + "readline_helpers.rs", ("readline",)),
    (RT + "node_repl.rs", ("repl",)),
    (RT + "node_sea.rs", ("sea",)),
    (RT + "node_stream*", ("stream",)),
    (RT + "node_submodules/stream_promises*", ("stream",)),
    (RT + "stream_hooks.rs", ("stream",)),
    (RT + "utf8_stream_decoder.rs", ("string_decoder",)),
    (RT + "node_test*", ("test",)),
    (RT + "node_submodules/test*", ("test",)),
    (RT + "timer/*", ("timers",)),
    (RT + "timer.rs", ("timers",)),
    (RT + "node_submodules/timers.rs", ("timers",)),
    (RT + "tls.rs", ("tls",)),
    (RT + "tls/*", ("tls",)),
    (RT + "node_submodules/trace_events.rs", ("trace_events",)),
    (RT + "tty*", ("tty",)),
    (RT + "pty/*", ("tty",)),
    (RT + "url/*", ("url",)),
    (RT + "url_hooks.rs", ("url",)),
    (RT + "util_*.rs", ("util", "sys")),
    (RT + "object/util_types.rs", ("util", "sys")),
    (RT + "object/assert.rs", ("assert",)),
    (RT + "v8.rs", ("v8",)),
    (RT + "node_v8*", ("v8",)),
    (RT + "node_vm*", ("vm",)),
    (RT + "wasi*", ("wasi",)),
    (RT + "messaging*", ("worker_threads",)),
    (RT + "object/native_module/worker_environment.rs", ("worker_threads",)),
    (RT + "node_submodules/zlib.rs", ("zlib",)),
    (STD + "crypto*", ("crypto",)),
    (STD + "webcrypto*", ("crypto", "globals")),
    (STD + "domain.rs", ("domain",)),
    (STD + "events*", ("events",)),
    (STD + "fetch*", ("fetch",)),
    (STD + "querystring.rs", ("querystring",)),
    (STD + "readline/*", ("readline",)),
    (STD + "sqlite*", ("sqlite",)),
    (STD + "streams*", ("stream", "fetch", "globals")),
    (STD + "string_decoder.rs", ("string_decoder",)),
    (STD + "tls*", ("tls", "https")),
    (STD + "turnloop_tls_client.rs", ("tls", "https")),
    (STD + "vm.rs", ("vm",)),
    (STD + "worker_threads*", ("worker_threads",)),
    ("crates/perry-ext-http/*", ("http", "https", "http2")),
    ("crates/perry-ext-net/*", ("net", "tls")),
    ("crates/perry-ext-zlib/*", ("zlib",)),
    ("crates/perry-http-server/*", ("http",)),
    ("crates/perry-tls-session/*", ("fetch", "https", "tls")),
    ("crates/perry-tls-turnloop/*", ("http2", "tls")),
    (TABLE + "async_hooks.rs", ("async_hooks",)),
    (TABLE + "node_core/assert.rs", ("assert",)),
    (TABLE + "node_core/dgram_fs_os.rs", ("dgram", "fs", "os", "tty", "wasi")),
    (TABLE + "node_core/inspector_vm.rs", ("inspector", "inspector-promises", "vm")),
    (TABLE + "node_core/module_sea_tls_test.rs", ("module", "sea", "test", "tls")),
    (TABLE + "node_core/url_punycode_console.rs", ("url", "punycode", "console")),
    (TABLE + "node_core/util_buffer.rs", ("util", "sys", "buffer")),
    (TABLE + "node_core_util.rs", ("util", "sys")),
    (TABLE + "node_core_process.rs", ("process",)),
    (TABLE + "node_dns.rs", ("dns",)),
    (TABLE + "node_domain.rs", ("domain",)),
    (TABLE + "http_client.rs", ("http", "https")),
    (TABLE + "http_server.rs", ("http",)),
    (TABLE + "http_http2.rs", ("http2",)),
    (TABLE + "net_*.rs", ("net", "tls")),
    (TABLE + "tls_events.rs", ("tls",)),
    (TABLE + "databases.rs", ("sqlite",)),
    (CG + "lower_call/extern_timers.rs", ("timers",)),
    (CG + "expr/child_proc.rs", ("child_process",)),
    (CG + "expr/fs_await.rs", ("fs", "fs-promises")),
    (CG + "expr/calls/fs.rs", ("fs", "fs-promises")),
    (CG + "expr/url_*.rs", ("url",)),
    (CG + "expr/v8_interop.rs", ("v8",)),
    (CG + "expr/worker_new.rs", ("worker_threads",)),
    (CG + "expr/calls/crypto_*.rs", ("crypto",)),
    (HIR + "crypto.rs", ("crypto",)),
    (HIR + "stream.rs", ("stream",)),
    (HIR + "url_search_params.rs", ("url",)),
]


def owners(path: str) -> set[str]:
    return {m for pattern, modules in OWNERS if fnmatch.fnmatchcase(path, pattern) for m in modules}


def selectors(root: Path, changed: list[str]) -> list[str]:
    picked: set[str] = set()
    cases: list[tuple[str, Path]] = []
    for rel in changed:
        path = root / rel
        if path.is_file() and parity.is_case(root, path):
            picked.add(rel)
        elif rel.startswith(("test-parity/expected/", "test-parity/expected-exit/")):
            # parity.expected_for looks an expectation up by case id, then by basename.
            key = rel.split("/", 2)[-1].rsplit(".", 1)[0]
            cases = cases or parity.select(root)
            picked.update(path.relative_to(root).as_posix() for cid, path in cases
                          if key in (cid, cid.rsplit("/", 1)[-1]))
        elif rel.startswith("test-parity/snapshots/") and rel.endswith(".json"):
            subject = rel.split("/", 3)[-1][:-len(".json")]
            if subject != "test-files" and (root / "test-parity" / subject).is_dir():
                picked.add(subject)
        else:
            picked.update(f"node-suite/{m}" for m in owners(rel)
                          if (root / "test-parity" / "node-suite" / m).is_dir())
    # A whole subject covers any of its cases also named on their own.
    subjects = {p for p in picked if not p.endswith((".ts", ".cts", ".mts", ".cjs"))}
    return sorted(p for p in picked if p in subjects
                  or not any(p.startswith(f"test-parity/{s}/") for s in subjects))


def changed_paths(base: str) -> list[str]:
    out = subprocess.run(["git", "diff", "--name-only", f"{base}...HEAD"], cwd=ROOT,
                         check=True, capture_output=True, text=True).stdout
    return [line for line in out.splitlines() if line]


def new_gap_files(base: str) -> list[str]:
    out = subprocess.run(["git", "diff", "--name-only", "--diff-filter=A", base, "HEAD", "--",
                          "test-files/test_gap_*"], cwd=ROOT, check=True,
                         capture_output=True, text=True).stdout
    return out.split()


def self_test() -> int:
    tracked = subprocess.run(["git", "ls-files", "crates"], cwd=ROOT, check=True,
                             capture_output=True, text=True).stdout.splitlines()
    problems = []
    for pattern, modules in OWNERS:
        if not any(fnmatch.fnmatchcase(f, pattern) for f in tracked):
            problems.append(f"row {pattern!r} matches no tracked file")
        problems += [f"row {pattern!r} names missing subject node-suite/{m}" for m in modules
                     if not (ROOT / "test-parity" / "node-suite" / m).is_dir()]
    case = "test-parity/node-suite/path/apply-call/basic.ts"
    examples = {
        (RT + "fs/mod.rs",): ["node-suite/fs", "node-suite/fs-promises"],
        ("crates/perry-runtime/src/gc/mod.rs",): [],
        ("README.md",): [],
        (case, RT + "path/mod.rs"): ["node-suite/path"],
    }
    examples[(case,)] = [case]
    examples[("test-parity/expected/array-generic-copywithin.txt",)] = [
        "test-parity/node-suite/globals/array-generic-copywithin.ts"]
    for changed, want in examples.items():
        got = selectors(ROOT, list(changed))
        if got != want:
            problems.append(f"{list(changed)} selected {got}, want {want}")
    for problem in problems:
        print(f"test_owners --self-test: {problem}", file=sys.stderr)
    print(f"test_owners --self-test: {len(OWNERS)} rows, "
          + ("FAIL" if problems else "all rows live, examples hold"))
    return 1 if problems else 0


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--base")
    ap.add_argument("--self-test", action="store_true")
    ap.add_argument("--no-new-gap-vs", metavar="REF")
    args = ap.parse_args(argv)
    if args.self_test:
        return self_test()
    if args.no_new_gap_vs:
        added = new_gap_files(args.no_new_gap_vs)
        for path in added:
            print(f"{path}: test_gap_* takes no new files. Put a new program case under "
                  "test-parity/<root>/<subject>/: node-suite/<module>, language/<area>, "
                  "typescript/<feature>, perry/<api> or npm/<package> "
                  "(docs/src/testing/test-structure.md, \"Where a test goes\").", file=sys.stderr)
        return 1 if added else 0
    changed = changed_paths(args.base) if args.base else [l.strip() for l in sys.stdin if l.strip()]
    print("\n".join(selectors(ROOT, changed)))
    return 0


if __name__ == "__main__":
    sys.exit(main())
