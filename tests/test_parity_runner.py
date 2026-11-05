#!/usr/bin/env python3
"""Behaviour of `scripts/parity.py`, driven through its CLI and public helpers.

End-to-end cases run against a temporary repository with stand-in `node` and
`perry` executables (`tests/fixtures/parity_runner/`), so every verdict is
produced by the real compile/run/compare path, not by a mocked judge.
"""

import json
import re
import shlex
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
import wasi_run  # noqa: E402

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
    """Case ids are snapshot keys: they stay the ids the replaced shell runner reported."""

    def legacy_ids(self, suite):
        # The replaced runner's discovery and id derivation, verbatim.
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
        self.assertTrue((ROOT / "test-files").is_dir())
        self.assertEqual(sorted(ids), self.legacy_ids("all"))

    def test_node_suite_ts_ids_match_legacy(self):
        ids = [case_id for case_id, path in parity.select(ROOT, ["node-suite"])
               if path.suffix == ".ts"]
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

    def test_subject_roots_are_discovered_like_node_suite(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = make_repo(Path(tmp))
            write(root / "test-parity/language/string/a.ts")
            write(root / "test-parity/language/string/fixtures/dep.ts")
            write(root / "test-parity/typescript/enum/x/b.ts")
            write(root / "test-parity/perry/thread/c.ts")
            write(root / "test-parity/npm/zod/d.ts")
            write(root / "test-parity/expected/language/string/a.txt")

            def ids(*selectors, **kw):
                return [i for i, _ in parity.select(root, list(selectors), **kw)]

            self.assertEqual(ids("language/string"), ["language/string/a"])
            self.assertEqual(ids("typescript"), ["typescript/enum/x/b"])
            self.assertEqual(ids(str(root / "test-parity/perry/thread/c.ts")), ["perry/thread/c"])
            self.assertEqual(ids(suite="all"), ["language/string/a", "npm/zod/d",
                                                "perry/thread/c", "typescript/enum/x/b"])
            self.assertEqual(parity.subject_of("typescript/enum/x/b"), "typescript/enum")

    def test_subjects_preserve_legacy_module_formats(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = make_repo(Path(tmp)).resolve()
            names = ["test_gap_ts.ts", "test_gap_cts.cts", "test_gap_mts.mts", "test_gap_cjs.cjs"]
            legacy = ["test_gap_ts", "test_gap_cts", "test_gap_mts", "test_gap_cjs.cjs"]
            for name in names:
                write(root / "test-files" / name)
            self.assertEqual(sorted(cid for cid, _ in parity.select(root, ["test-files"])),
                             sorted(legacy))
            for subject_root in parity.SUBJECT_ROOTS:
                subject = root / "test-parity" / subject_root / "formats"
                for name in names:
                    write(subject / name)
                    write(subject / "fixtures" / name)
                write(subject / "helper.cjs")
                write(subject / "test_helper.js")
                selected = parity.select(root, [f"{subject_root}/formats"], filters=["test_gap_"])
                self.assertEqual([cid for cid, _ in selected],
                                 sorted(f"{subject_root}/formats/{cid}" for cid in legacy))
                self.assertEqual(len(parity.select(root, [f"{subject_root}/formats"])), 4)
                for cid, path in selected:
                    self.assertEqual(parity.case_id(root, path), cid)

    def test_subject_of(self):
        self.assertEqual(parity.subject_of("test_gap_x"), "test-files")
        self.assertEqual(parity.subject_of("node-suite/fs/promises/a"), "node-suite/fs")


class GapLane(Repo):
    def workflow_args(self):
        workflow = (ROOT / ".github/workflows/test.yml").read_text()
        match = re.search(r"args=\((run --suite [^\n]+)\)", workflow)
        self.assertIsNotNone(match)
        return [arg.replace("$GAP_SHARD/$GAP_TOTAL", "1/1")
                for arg in shlex.split(match.group(1))[1:]]

    def test_moving_cases_to_subjects_preserves_gap_lane_verdicts(self):
        retained = ["test_gap_cts.cts", "test_gap_mts.mts", "test_gap_cjs.cjs"]
        for name in retained:
            write(self.root / "test-files" / name)
        write(self.root / "test-files/test_parity_excluded.ts")
        mapping = {}
        for subject_root in parity.SUBJECT_ROOTS:
            old_id = f"test_gap_{subject_root}"
            mapping[old_id] = f"{subject_root}/subject/{old_id}"
            write(self.root / "test-files" / f"{old_id}.ts")

        before_code, before = self.run_cli(*self.workflow_args())
        self.assertEqual(before_code, 0)
        self.assertEqual(len(before["results"]), len(retained) + len(mapping))
        for old_id, new_id in mapping.items():
            old_path = self.root / "test-files" / f"{old_id}.ts"
            new_path = self.root / "test-parity" / f"{new_id}.ts"
            new_path.parent.mkdir(parents=True)
            old_path.rename(new_path)
            write(new_path.parent / "fixtures/test_gap_helper.ts")
            write(new_path.parent / "test_parity_excluded.ts")

        after_code, after = self.run_cli(*self.workflow_args())
        self.assertEqual(after_code, before_code)
        self.assertEqual(self.statuses(after), {
            mapping.get(cid, cid): status for cid, status in self.statuses(before).items()
        })

    def test_worker_subject_paths_are_trackable(self):
        shutil.copyfile(ROOT / ".gitignore", self.root / ".gitignore")
        env = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
        env.update(GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull)
        subprocess.run(["git", "init", "--quiet", "--initial-branch=main"],
                       cwd=self.root, env=env, check=True, capture_output=True, timeout=10)
        paths = {
            "worker": True,
            "examples/worker/source.ts": True,
            "test-parity/perry/worker/test_gap_worker_not_compiled.ts": False,
            "test-parity/expected/perry/worker/test_gap_worker_not_compiled.txt": False,
            "test-parity/perry/worker/worker": True,
        }
        for relative, ignored in paths.items():
            write(self.root / relative)
            with self.subTest(path=relative):
                result = subprocess.run(
                    ["git", "-c", f"core.excludesFile={os.devnull}",
                     "check-ignore", "--no-index", "--quiet", relative],
                    cwd=self.root, env=env, capture_output=True, timeout=10)
                self.assertEqual(result.returncode, 0 if ignored else 1, result.stderr)

    def test_gap_snapshot_artifact_includes_subject_snapshots(self):
        workflow = (ROOT / ".github/workflows/test.yml").read_text()
        upload = workflow.split("- name: Upload re-baselined gap snapshot", 1)[1]
        upload = upload.split("- name:", 1)[0]
        self.assertRegex(upload, r"path: test-parity/snapshots/linux-fast/\*\*/\*\.json")


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


class WasiGapSources(unittest.TestCase):
    """WASI's fixed sample IDs survive subject relocation without compiling."""

    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="wasi-gap-sources-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)

    def test_subject_sources_keep_sample_order_and_artifact_basenames(self):
        paths = {
            "test_gap_array_methods": "test-parity/language/array/test_gap_array_methods.ts",
            "test_gap_string_methods": "test-files/test_gap_string_methods.ts",
            "test_gap_json_advanced": "test-parity/language/json/test_gap_json_advanced.ts",
        }
        for path in reversed(list(paths.values())):
            write(self.root / path)
        write(self.root / "test-parity/language/array/fixtures/test_gap_array_methods.ts")
        sources = wasi_run.gap_sources(self.root, ["json_advanced", "array_methods", "string_methods", "array"])
        self.assertEqual(sources, [self.root / path for path in paths.values()])
        self.assertEqual([path.stem for path in sources], [
            "test_gap_array_methods", "test_gap_string_methods", "test_gap_json_advanced",
        ])

    def test_flat_sources_keep_the_complete_fixed_sample_set(self):
        names = [
            "test_gap_array_methods", "test_gap_string_methods", "test_gap_closures",
            "test_gap_class_advanced", "test_gap_map_set_extended", "test_gap_json_advanced",
            "test_gap_regexp_advanced", "test_gap_async_advanced", "test_gap_error_extensions",
            "test_gap_object_methods",
        ]
        paths = [write(self.root / f"test-files/{name}.ts") for name in names]
        write(self.root / "test-files/test_gap_array_methods_extra.ts")
        self.assertEqual(wasi_run.gap_sources(self.root), paths)
        self.assertEqual(wasi_run.gap_sources(self.root, ["array_methods"]), paths[:1])

    def test_filters_ignore_unselected_missing_or_ambiguous_samples(self):
        source = write(self.root / "test-parity/language/string/test_gap_string_methods.ts")
        write(self.root / "test-files/test_gap_json_advanced.ts")
        write(self.root / "test-parity/language/json/test_gap_json_advanced.ts")
        self.assertEqual(wasi_run.gap_sources(self.root, ["string"]), [source])
        self.assertEqual(wasi_run.gap_sources(self.root, ["absent"]), [])
        self.assertEqual(wasi_run.gap_sources(self.root, ["string*"]), [])

    def test_missing_selected_source_keeps_legacy_locator_without_changing_sources(self):
        write(self.root / "test-parity/language/json/fixtures/test_gap_json_advanced.ts")
        source = write(self.root / "test-parity/language/string/test_gap_string_methods.ts")
        before = {path.relative_to(self.root): path.read_bytes()
                  for path in self.root.rglob("*") if path.is_file()}
        self.assertEqual(wasi_run.gap_sources(self.root, ["json_advanced", "string_methods"]),
                         [source, self.root / "test-files/test_gap_json_advanced.ts"])
        self.assertEqual(before, {path.relative_to(self.root): path.read_bytes()
                                  for path in self.root.rglob("*") if path.is_file()})

    def test_existing_legacy_source_takes_precedence_over_subject_duplicates(self):
        legacy = write(self.root / "test-files/test_gap_json_advanced.ts")
        write(self.root / "test-parity/language/json/test_gap_json_advanced.ts")
        self.assertEqual(wasi_run.gap_sources(self.root, ["json_advanced"]), [legacy])

    def test_canonical_only_duplicate_basename_fails_closed(self):
        write(self.root / "test-parity/language/json/test_gap_json_advanced.ts")
        write(self.root / "test-parity/language/object/test_gap_json_advanced.ts")
        with self.assertRaisesRegex(ValueError, "ambiguous.*test_gap_json_advanced"):
            wasi_run.gap_sources(self.root, ["json_advanced"])


class SourcePatterns(Repo):
    def source_cli(self, *args):
        return subprocess.run(
            [sys.executable, str(ROOT / "scripts/gc_corpus_sources.py"),
             "--root", str(self.root), *args], capture_output=True, text=True, timeout=10)

    def test_patterns_preserve_order_repeats_and_subject_sources(self):
        write(self.root / "test-files/test_gap_gc_a.ts")
        write(self.root / "test-parity/language/object/test_gap_gc_b.ts")
        write(self.root / "test-parity/language/class/test_gap_class_c.ts")
        write(self.root / "test-parity/language/object/fixtures/test_gap_gc_fake.ts")
        result = self.source_cli("--pattern", "test_gap_gc_*.ts",
                                 "--pattern", "test_gap_class*.ts",
                                 "--pattern", "test_gap_gc_a.ts")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.splitlines(), [
            "test_gap_gc_a\ttest-files/test_gap_gc_a.ts",
            "test_gap_gc_b\ttest-parity/language/object/test_gap_gc_b.ts",
            "test_gap_class_c\ttest-parity/language/class/test_gap_class_c.ts",
            "test_gap_gc_a\ttest-files/test_gap_gc_a.ts",
        ])
        self.assertEqual(result.stderr, "")


    def test_missing_pattern_fails_before_emitting_sources(self):
        write(self.root / "test-files/test_gap_gc_a.ts")
        before = {path.relative_to(self.root): path.read_bytes()
                  for path in self.root.rglob("*") if path.is_file()}
        result = self.source_cli("--pattern", "test_gap_gc_*.ts",
                                 "--pattern", "test_gap_missing*.ts")
        self.assertEqual(result.returncode, 3, result.stderr)
        self.assertEqual(result.stdout, "")
        self.assertEqual(result.stderr,
                         "::error::pattern 'test_gap_missing*.ts' matched nothing; it is stale. "
                         "Remove it or fix it.\n")
        self.assertEqual(before, {path.relative_to(self.root): path.read_bytes()
                                  for path in self.root.rglob("*") if path.is_file()})


    def test_optional_registration_patterns_can_be_empty(self):
        result = self.source_cli("--pattern", "test_gap_repsel_*.ts",
                                 "--pattern", "test_gap_specabi_*.ts",
                                 "--allow-empty-patterns")
        self.assertEqual((result.returncode, result.stdout, result.stderr), (0, "", ""))


class ArchivePreflight(Repo):
    def test_no_auto_archive_guard_follows_case_basename(self):
        stem = "test_gap_11301_namespace_member_values"
        locations = [
            (stem, f"test-files/{stem}.ts"),
            (f"typescript/namespace/{stem}", f"test-parity/typescript/namespace/{stem}.cts"),
            (f"node-suite/globals/{stem}", f"test-parity/node-suite/globals/{stem}.ts"),
            ("node-suite/http/test_regular", "test-parity/node-suite/http/test_regular.ts"),
        ]
        for cid, relative in locations:
            path = write(self.root / relative)
            with self.subTest(case_id=cid):
                with self.assertRaises(parity.HarnessError) as error:
                    parity.check_toolchain(self.root, "fast", [(cid, path)])
                for stem in ("perry_ext_http", "perry_ext_net", "perry_ext_ws"):
                    self.assertIn(stem, str(error.exception))

    def test_regular_archive_requirements_stay_unchanged(self):
        for cid, relative in [
                ("test_regular", "test-files/test_regular.ts"),
                ("language/object/test_regular", "test-parity/language/object/test_regular.ts"),
                ("node-suite/crypto/test_regular", "test-parity/node-suite/crypto/test_regular.ts")]:
            path = write(self.root / relative)
            with self.subTest(case_id=cid):
                _, runtime = parity.check_toolchain(self.root, "fast", [(cid, path)])
                self.assertEqual(runtime, self.root / "rt")


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


class Companions(unittest.TestCase):
    """A case that names a companion server's port gets that server, once per run."""

    def started(self, sources: dict) -> list:
        with tempfile.TemporaryDirectory() as tmp:
            files = Path(tmp) / "test-files"
            files.mkdir()
            cases = []
            for name, text in sources.items():
                (files / f"{name}.ts").write_text(text)
                cases.append((name, files / f"{name}.ts"))
            with mock.patch.object(parity.subprocess, "Popen") as popen, \
                    mock.patch.object(parity.socket, "create_connection"):
                parity.start_companions(Path(tmp), cases)
            return [Path(call.args[0][1]).name for call in popen.call_args_list]

    def test_a_case_naming_the_echo_port_gets_the_echo_server(self):
        echo = "createConnection(17891 as never, '127.0.0.1' as never);"
        self.assertEqual(self.started({"test_issue_1_a": echo, "test_issue_2_b": echo}),
                         ["test_net_echo_server.py"])

    def test_a_case_naming_no_port_starts_nothing(self):
        self.assertEqual(self.started({"test_gap_x": "console.log(1);"}), [])


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
