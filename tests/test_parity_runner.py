#!/usr/bin/env python3
"""Behaviour of `scripts/parity.py`, driven through its CLI and public helpers.

End-to-end cases run against a temporary repository with stand-in `node` and
`perry` executables (`tests/fixtures/parity_runner/`), so every verdict is
produced by the real compile/run/compare path, not by a mocked judge.
"""

import json
import re
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
FIXTURES = HERE / "fixtures" / "parity_runner"
sys.path.insert(0, str(ROOT / "scripts"))

import parity  # noqa: E402  (needs the sys.path entry above)
import parity_normalize  # noqa: E402

LIB_SUFFIX = ".lib" if os.name == "nt" else ".a"
LIB_PREFIX = "" if os.name == "nt" else "lib"


def make_repo(tmp: Path) -> Path:
    (tmp / "test-files").mkdir()
    (tmp / "test-parity" / "node-suite").mkdir(parents=True)
    (tmp / ".node-version").write_text("1.0.0\n")
    runtime = tmp / "rt"
    runtime.mkdir()
    for stem in ("perry_runtime", "perry_stdlib"):
        (runtime / f"{LIB_PREFIX}{stem}{LIB_SUFFIX}").write_text("")
    (runtime / f"{LIB_PREFIX}perry_runtime{LIB_SUFFIX}.gc-effects").write_text("")
    return tmp


def write(path: Path, text: str = "") -> Path:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text)
    return path


class Repo(unittest.TestCase):
    """A scratch repository plus the environment the runner reads."""

    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp(prefix="parity-runner-test-"))
        self.root = make_repo(self.tmp)
        env = {
            "PERRY_BIN": str(FIXTURES / "fake_perry.py"),
            "PERRY_RUNTIME_DIR": str(self.root / "rt"),
            "NODE_BIN": str(FIXTURES / "fake_node.py"),
            "PERRY_HOST_PLATFORM": "linux",
        }
        patcher = mock.patch.dict(os.environ, env)
        patcher.start()
        self.addCleanup(patcher.stop)
        self.addCleanup(shutil.rmtree, self.tmp, True)

    def run_cli(self, *argv):
        report = self.root / "report.json"
        with mock.patch("sys.stdout"), mock.patch("sys.stderr"):
            code = parity.main(["run", "--mode", "fast", "--jobs", "2",
                                "--report", str(report), *argv], root=self.root)
        data = json.loads(report.read_text()) if report.exists() else None
        return code, data

    def statuses(self, data):
        return {entry["id"]: entry["status"] for entry in data["results"]}


class Discovery(unittest.TestCase):
    """Case ids must stay the ids run_parity_tests.sh reports."""

    def legacy_ids(self, suite):
        # The discovery and id derivation of run_parity_tests.sh, verbatim.
        script = r'''
        cd "$1"
        if [ "$2" = all ]; then
          find test-files -maxdepth 1 -type f \( -name '*.ts' -o -name '*.cts' -o -name '*.mts' -o -name 'test_*.cjs' \) \
            | while read -r f; do basename "$f" | sed -E 's/\.(m|c)?ts$//'; done
        else
          find test-parity/node-suite -type f -name '*.ts' -not -path '*/fixtures/*' \
            | while read -r f; do r="${f#test-parity/node-suite/}"; echo "node-suite/${r%.ts}"; done
        fi
        '''
        out = subprocess.run(["bash", "-c", script, "_", str(ROOT), suite],
                             capture_output=True, text=True, check=True).stdout
        return sorted(out.split())

    def test_test_files_ids_match_legacy(self):
        ids = [case_id for case_id, _ in parity.select(ROOT, ["test-files"])]
        self.assertGreater(len(ids), 1000)
        self.assertEqual(sorted(ids), self.legacy_ids("all"))

    def test_node_suite_ids_match_legacy(self):
        ids = [case_id for case_id, _ in parity.select(ROOT, ["node-suite"])]
        self.assertGreater(len(ids), 1000)
        self.assertEqual(sorted(ids), self.legacy_ids("node-suite"))

    def test_selectors_subject_dir_and_file(self):
        with tempfile.TemporaryDirectory() as td:
            root = make_repo(Path(td))
            write(root / "test-files/test_a.ts")
            write(root / "test-files/test_b.cts")
            write(root / "test-files/test_c.cjs")
            write(root / "test-files/helper.cjs")
            write(root / "test-files/sub/test_d.ts")
            write(root / "test-parity/node-suite/fs/a.ts")
            write(root / "test-parity/node-suite/fs/x/b.ts")
            write(root / "test-parity/node-suite/fs/fixtures/dep.ts")
            write(root / "test-parity/node-suite/path/a.ts")

            def ids(*selectors, **kw):
                return [i for i, _ in parity.select(root, list(selectors), **kw)]

            self.assertEqual(ids("test-files"), ["test_a", "test_b", "test_c.cjs"])
            self.assertEqual(ids("node-suite/fs"), ["node-suite/fs/a", "node-suite/fs/x/b"])
            self.assertEqual(ids(str(root / "test-parity/node-suite/fs/x")), ["node-suite/fs/x/b"])
            self.assertEqual(ids(str(root / "test-files/test_b.cts")), ["test_b"])
            self.assertEqual(len(ids(suite="all")), 6)
            self.assertEqual(ids(suite="node-suite", filters=["/a"]),
                             ["node-suite/fs/a", "node-suite/path/a"])
            with self.assertRaises(parity.HarnessError):
                ids("node-suite/nope")

    def test_sharding_is_round_robin_partition(self):
        full = [i for i, _ in parity.select(ROOT, ["node-suite/path"])]
        shards = [[i for i, _ in parity.select(ROOT, ["node-suite/path"], shard=(n, 3))]
                  for n in (1, 2, 3)]
        self.assertEqual(shards[0], full[0::3])
        self.assertEqual(shards[1], full[1::3])
        self.assertEqual(sorted(sum(shards, [])), sorted(full))

    def test_subject_of(self):
        self.assertEqual(parity.subject_of("test_gap_x"), "test-files")
        self.assertEqual(parity.subject_of("node-suite/fs/promises/a"), "node-suite/fs")


class Headers(unittest.TestCase):
    def test_parses_every_header(self):
        text = (
            "// parity-argv: a  b\n"
            "  // parity-node-argv: --expose-gc --no-warnings\n"
            "// parity-env: A=1 B=two\n"
            "// parity-skip: needs a server\n"
            "// parity-argv: ignored second\n"
        )
        h = parity.parse_headers(text)
        self.assertEqual(h["argv"], ["a", "b"])
        self.assertEqual(h["node_argv"], ["--expose-gc", "--no-warnings"])
        self.assertEqual(h["env"], {"A": "1", "B": "two"})
        self.assertEqual(h["skip"], "needs a server")
        self.assertFalse(h["rlimit"])
        self.assertTrue(parity.parse_headers("// parity-rlimit-as-mb: 64\n")["rlimit"])
        self.assertIsNone(parity.parse_headers("x\n")["skip"])


class Expected(unittest.TestCase):
    def test_case_id_path_wins_over_basename(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            write(root / "test-parity/expected/a.txt", "base")
            write(root / "test-parity/expected/node-suite/m/a.txt", "byid")
            write(root / "test-parity/expected-exit/a.txt", " 3\n")
            path, code = parity.expected_for(root, "node-suite/m/a")
            self.assertEqual(path.read_text(), "byid")
            self.assertEqual(code, 3)
            path, _ = parity.expected_for(root, "node-suite/other/a")
            self.assertEqual(path.read_text(), "base")
            self.assertEqual(parity.expected_for(root, "node-suite/m/b"), (None, 0))


class Verdicts(Repo):
    def test_every_status(self):
        cases = {
            "test_pass": "// node-out: hi\n// perry-out: hi\n",
            "test_bool_folds": "// node-out: true\n// perry-out: 1\n",
            "test_mismatch": "// node-out: hi\n// perry-out: ho\n",
            "test_exit": "// node-out: hi\n// perry-out: hi\n// perry-exit: 3\n",
            "test_node_throw": "// node-out: boom\n// node-exit: 1\n// perry-out: boom\n// perry-exit: 1\n",
            "test_node_crash": "// node-signal\n",
            "test_crash": "// node-out: hi\n// perry-out: hi\n// perry-signal\n",
            "test_timeout": "// perry-sleep: 5\n",
            "test_skip": "// parity-skip: needs a server\n",
            "test_timer": "",
            "test_cfail": "// perry-compile-fail\n",
            "test_env": "// parity-env: PARITY_PROBE=probe\n// node-out: probe\n",
        }
        for name, text in cases.items():
            write(self.root / "test-files" / f"{name}.ts", text)
        with mock.patch.object(parity, "RUN_TIMEOUT", 1):
            code, data = self.run_cli("--unpinned-oracle")
        self.assertEqual(code, 1)
        self.assertEqual(self.statuses(data), {
            "test_pass": "pass",
            "test_bool_folds": "pass",
            "test_mismatch": "parity_fail",
            "test_exit": "parity_fail",
            "test_node_throw": "pass",
            "test_node_crash": "node_fail",
            "test_crash": "crash",
            "test_timeout": "crash",
            "test_skip": "skipped",
            "test_timer": "skipped",
            "test_cfail": "compile_fail",
            "test_env": "pass",
        })
        self.assertEqual(data["mode"], "fast")
        self.assertEqual(data["platform"], "linux")
        self.assertEqual(data["summary"]["crash_fail"], 2)
        self.assertEqual(sorted(data["failures"]["crash"]), ["test_crash", "test_timeout"])

    def test_expected_output_replaces_node(self):
        write(self.root / "test-files/test_x.ts", "// node-out: node\n// perry-out: perry\n// perry-exit: 2\n")
        write(self.root / "test-parity/expected/test_x.txt", "perry\n")
        write(self.root / "test-parity/expected-exit/test_x.txt", "2\n")
        code, data = self.run_cli()
        self.assertEqual(self.statuses(data), {"test_x": "pass"})
        self.assertEqual(code, 0)

    def test_zero_cases_is_harness_error(self):
        write(self.root / "test-files/test_a.ts")
        code, _ = self.run_cli("--filter", "nothing-matches")
        self.assertEqual(code, 2)

    def test_missing_gc_sidecar_is_harness_error(self):
        write(self.root / "test-files/test_a.ts")
        (self.root / f"rt/{LIB_PREFIX}perry_runtime{LIB_SUFFIX}.gc-effects").unlink()
        self.assertEqual(self.run_cli()[0], 2)

    def test_node_version_mismatch_refused_unless_overridden(self):
        write(self.root / "test-files/test_a.ts")
        (self.root / ".node-version").write_text("9.9.9\n")
        code, _ = self.run_cli()
        self.assertEqual(code, 2)
        code, data = self.run_cli("--unpinned-oracle")
        self.assertEqual(code, 0)
        self.assertTrue(data["oracle_override"])

    def test_snapshot_gates_both_directions(self):
        write(self.root / "test-files/test_a.ts", "// node-out: a\n// perry-out: b\n")
        write(self.root / "test-files/test_b.ts")
        code, _ = self.run_cli()
        self.assertEqual(code, 1)  # test_a regressed: no snapshot means all pass
        code, _ = self.run_cli("--update")
        self.assertEqual(code, 0)
        snap = self.root / "test-parity/snapshots/linux-fast/test-files.json"
        tests = json.loads(snap.read_text())["tests"]
        self.assertEqual(tests["test_a"]["status"], "parity_fail")
        self.assertIsNone(tests["test_a"]["issue"])
        self.assertEqual(tests["test_a"]["category"], "untriaged")
        self.assertEqual(self.run_cli()[0], 0)
        write(self.root / "test-files/test_a.ts")  # fixed: the entry is now stale
        self.assertEqual(self.run_cli()[0], 1)
        self.assertEqual(self.run_cli("--update")[0], 0)
        self.assertNotIn("test_a", json.loads(snap.read_text())["tests"])

    def test_update_never_parks_a_crash(self):
        write(self.root / "test-files/test_a.ts", "// perry-signal\n")
        code, _ = self.run_cli("--update")
        self.assertEqual(code, 1)
        snap = self.root / "test-parity/snapshots/linux-fast/test-files.json"
        self.assertFalse(snap.exists() and "test_a" in json.loads(snap.read_text())["tests"])

    def test_parked_crash_in_snapshot_is_harness_error(self):
        write(self.root / "test-files/test_a.ts")
        write(self.root / "test-parity/snapshots/linux-fast/test-files.json",
              json.dumps({"tests": {"test_a": {"status": "crash"}}}))
        self.assertEqual(self.run_cli()[0], 2)


class Snapshots(unittest.TestCase):
    def test_diff_kinds(self):
        entries = {"b": {"status": "parity_fail"}, "c": {"status": "parity_fail"}}
        got = parity.diff({"a": "crash", "b": "pass", "c": "compile_fail", "d": "pass"}, entries)
        self.assertEqual(got, [("regression", "a", "pass", "crash"),
                               ("improvement", "b", "parity_fail", "pass"),
                               ("changed", "c", "parity_fail", "compile_fail")])

    def test_fold_keeps_provenance_and_unrun_entries(self):
        entries = {"b": {"status": "parity_fail", "issue": "42", "added": "2026-01-01",
                         "category": "bug-open", "reason": "why"},
                   "z": {"status": "skipped"}}
        folded, crashes = parity.fold({"b": "compile_fail", "c": "crash"}, entries,
                                      "2026-11-02", lambda _: True)
        self.assertEqual(folded["b"]["issue"], "42")
        self.assertEqual(folded["b"]["status"], "compile_fail")
        self.assertIn("z", folded)
        self.assertNotIn("c", folded)
        self.assertEqual(crashes, ["c"])

    def test_prune_resolves_every_case_extension(self):
        with tempfile.TemporaryDirectory() as td:
            root = make_repo(Path(td))
            write(root / "test-files/test_c.cts")
            write(root / "test-files/test_j.cjs")
            write(root / "test-parity/node-suite/fs/a.ts")
            for case_id in ("test_c", "test_j.cjs", "node-suite/fs/a"):
                self.assertTrue(parity.case_exists(root, case_id), case_id)
            self.assertFalse(parity.case_exists(root, "test_gone"))
            entries = {i: {"status": "parity_fail"} for i in ("test_c", "test_j.cjs", "test_gone")}
            folded, _ = parity.fold({}, entries, "2026-11-02",
                                    lambda i: parity.case_exists(root, i))
            self.assertEqual(sorted(folded), ["test_c", "test_j.cjs"])


class Accept(Repo):
    def report(self, name, results, platform="linux"):
        path = self.root / name
        path.write_text(json.dumps({"platform": platform, "results": [
            {"id": i, "status": s} for i, s in results.items()]}))
        return str(path)

    def accept(self, *argv):
        with mock.patch("sys.stdout"), mock.patch("sys.stderr"):
            return parity.main(["accept", *argv], root=self.root)

    def test_accept_merges_reports_per_subject(self):
        write(self.root / "test-files/test_a.ts")
        write(self.root / "test-parity/node-suite/fs/x.ts")
        one = self.report("one.json", {"test_a": "parity_fail", "test_b": "pass"})
        two = self.report("two.json", {"node-suite/fs/x": "node_fail"})
        self.assertEqual(self.accept(one, two, "--mode", "auto"), 0)
        base = self.root / "test-parity/snapshots/linux-auto"
        self.assertEqual(json.loads((base / "test-files.json").read_text())["tests"]["test_a"]["status"],
                         "parity_fail")
        self.assertEqual(json.loads((base / "node-suite/fs.json").read_text())["tests"]
                         ["node-suite/fs/x"]["status"], "node_fail")

    def test_accept_rejects_conflicting_duplicates(self):
        one = self.report("one.json", {"test_a": "parity_fail"})
        two = self.report("two.json", {"test_a": "pass"})
        self.assertEqual(self.accept(one, two, "--mode", "fast"), 2)
        self.assertFalse((self.root / "test-parity/snapshots").exists())


class Lanes(unittest.TestCase):
    def test_port_and_timing_cases_run_serially(self):
        self.assertTrue(parity.runs_serially("node-suite/timers/a", "", "fast"))
        self.assertTrue(parity.runs_serially("test_gap_x", 'import net from "node:net";', "fast"))
        self.assertFalse(parity.runs_serially("test_gap_x", "console.log(1);", "fast"))

    def test_fast_mode_ext_routed_cases_run_serially(self):
        zlib = 'import zlib from "node:zlib";'
        self.assertTrue(parity.runs_serially("test_gap_x", zlib, "fast"))
        self.assertFalse(parity.runs_serially("test_gap_x", zlib, "auto"))
        self.assertFalse(parity.runs_serially("node-suite/zlib/a", zlib, "fast"))


class CompileBudget(unittest.TestCase):
    """An auto-optimize compile may rebuild the runtime, so it gets the toolchain budget."""

    def test_budget_per_compile_kind(self):
        plain, toolchain, ordinary = "console.log(1);", parity.TOOLCHAIN_TIMEOUT, parity.COMPILE_TIMEOUT
        forced = sorted(parity.FORCE_NO_AUTO)[0]
        self.assertEqual(parity.compile_budget("test_gap_x", plain, auto=True, ext_routed=False), toolchain)
        self.assertEqual(parity.compile_budget("test_gap_x", plain, auto=False, ext_routed=False), ordinary)
        self.assertEqual(parity.compile_budget("test_gap_x", "new WebAssembly.Module(b);",
                                               auto=False, ext_routed=False), toolchain)
        self.assertEqual(parity.compile_budget(forced, plain, auto=False, ext_routed=True), toolchain)
        self.assertEqual(parity.compile_budget(forced, plain, auto=False, ext_routed=False), ordinary)


@unittest.skipIf(os.name == "nt" or not Path("/bin/dash").exists(), "needs a POSIX host with dash")
class ProcessBudget(unittest.TestCase):
    def test_fork_cap_applies_when_sh_is_dash(self):
        # Ubuntu's /bin/sh is dash, whose ulimit has no -u.
        with tempfile.TemporaryDirectory() as td:
            os.symlink("/bin/dash", Path(td) / "sh")
            env = {**os.environ, "PATH": f"{td}{os.pathsep}{os.environ['PATH']}"}
            with mock.patch.dict(os.environ, env):
                prefix = parity.process_budget()
                done = subprocess.run([*prefix, "true"], capture_output=True, text=True)
        self.assertEqual(done.returncode, 0, done.stderr)


class ReviewFixes(Repo):
    def test_compiler_crash_is_a_crash_not_a_compile_failure(self):
        write(self.root / "test-files" / "test_cc.ts", "// perry-compile-signal\n")
        code, data = self.run_cli("--unpinned-oracle")
        self.assertEqual(code, 1)
        self.assertEqual(self.statuses(data), {"test_cc": "crash"})

    def test_accept_refuses_a_report_from_an_unpinned_oracle(self):
        path = self.root / "r.json"
        path.write_text(json.dumps({"platform": "linux", "mode": "fast", "oracle_override": True,
                                    "results": [{"id": "test_a", "status": "parity_fail"}]}))
        with mock.patch("sys.stdout"), mock.patch("sys.stderr"):
            self.assertEqual(parity.main(["accept", str(path)], root=self.root), 2)
        self.assertFalse((self.root / "test-parity/snapshots").exists())

    def test_fast_preflight_needs_ext_archives_for_forced_no_auto_cases(self):
        name = sorted(parity.FORCE_NO_AUTO)[0]
        case = self.root / "test-files" / f"{name}.ts"
        write(case, 'import http from "node:http";\n')
        with self.assertRaises(parity.HarnessError):
            parity.check_toolchain(self.root, "fast", [(name, case)])

    def test_submodule_imports_and_nested_node_suite_cases_run_serially(self):
        self.assertTrue(parity.runs_serially(
            "test_gap_x", 'import { setTimeout } from "node:timers/promises";', "auto"))
        self.assertTrue(parity.runs_serially(
            "node-suite/process/ipc/child", 'import { fork } from "node:child_process";', "auto"))

    @unittest.skipIf(os.name == "nt", "no RLIMIT_NPROC on Windows")
    def test_fork_cap_headroom_scales_with_workers(self):
        ps = subprocess.CompletedProcess([], 0, stdout="1\n" * 10, stderr="")
        with mock.patch.object(parity.subprocess, "run", return_value=ps):
            caps = [int(re.search(r"ulimit -u (\d+)", parity.process_budget(jobs)[2]).group(1))
                    for jobs in (1, 4)]
        self.assertEqual(caps, [10 + 50, 10 + 200])


class UnifiedProfile(unittest.TestCase):
    def test_union_of_parity_and_node_suite_rules(self):
        text = (
            "▶ suite\n"
            "  ✔ case (1.25ms)\n"
            "  test at file:///repo/a.ts:3:5\n"
            "▶ suite (2.5ms)\n"
            "label: 1:02.345 (m:ss.mmm)\n"
            "    … 3 more identical frames\n"
            "true\n"
            "plain (5ms) text\n"
        )
        self.assertEqual(parity_normalize.normalize_unified_output(text), (
            "▶ suite\n"
            "  ✔ case (<duration>)\n"
            "  test at <location>\n"
            "▶ suite (<duration>)\n"
            "label: <dur:m>\n"
            "1\n"
            "plain (5ms) text\n"
        ))


if __name__ == "__main__":
    unittest.main()
