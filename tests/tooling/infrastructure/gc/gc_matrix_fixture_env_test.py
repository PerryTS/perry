#!/usr/bin/env python3
"""Exercise matrix env dispatch with command probes, never GC acceptance.

Copies the original suffix fixture verbatim. The probes do not compile or run
TypeScript and emit no GC counters: moving cells must stay UNVER. Receipts
check the environment actually delivered to each compile/run command.
"""

from __future__ import annotations

import json
import hashlib
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile



sys.path.insert(0, str(Path(__file__).resolve().parents[4] / 'scripts'))

from gc_corpus_sources import resolve_sources


ROOT = Path(__file__).resolve().parents[4]
TEST = "test_gap_gc_string_suffix_cursor"
ARMS = "loop_polls,safepoint_minor,shipped_default,wb_off"


def real_fixture(case_id: str) -> Path:
    return resolve_sources(ROOT, [case_id])[0][1]


def assignments(text: str) -> dict[str, str]:
    return dict(word.split("=", 1) for word in text.split())


def run_probe(root: Path, matrix: str, metadata: str | None = None) -> subprocess.CompletedProcess[str]:
    (root / "scripts/gc_repsel_matrix.sh").write_text(matrix)
    if metadata is not None:
        (root / f"tests/fixtures/programs/{TEST}.ts").write_text(metadata)
    env = {key: value for key, value in os.environ.items() if not key.startswith("PERRY_")}
    # The routing probes own a fake compiler under this temporary target.
    # A caller's Cargo target must never select the real compiler instead.
    env.update(CARGO_TARGET_DIR=str(root / "target"),
               PATH=str(root / "probes") + os.pathsep + env["PATH"],
               ROUTING_RECEIPTS=str(root / "receipts.jsonl"))
    receipts = root / "receipts.jsonl"
    receipts.unlink(missing_ok=True)
    return subprocess.run(
        ["bash", str(root / "scripts/gc_repsel_matrix.sh"), "--no-build", "--arms", ARMS,
         "--jobs", "1", "--defer-liveness", "--json", str(root / "report.json")],
        env=env, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=30,
    )


def check_receipts(root: Path, result: subprocess.CompletedProcess[str]) -> None:
    assert result.returncode == 0, result.stdout[-2000:]
    report = json.loads((root / "report.json").read_text())
    receipts = [json.loads(line) for line in (root / "receipts.jsonl").read_text().splitlines()]
    runs = [receipt for receipt in receipts if receipt["stage"] == "run"]
    assert len(runs) == len(report["cells"]) == 4
    for cell, receipt in zip(report["cells"], runs):
        compile_env, run_env = assignments(cell["compile_env"]), assignments(cell["run_env"])
        assert compile_env == receipt["compile_env"], (cell["arm"], "compile receipt", receipt)
        assert run_env == receipt["run_env"], (cell["arm"], "run receipt", receipt)
        if cell["arm"] == "loop_polls":
            for effective in (compile_env, run_env):
                assert effective["PERRY_GC_SCHEDULE_SEED"] == "10061"
                assert effective["PERRY_GC_SCHEDULE_ALLOC_KB"] == "0"
                assert effective["PERRY_GC_PROTECT_FROMSPACE"] == "1"
                assert effective["PERRY_GC_VERIFY_EVACUATION"] == "1"
            assert compile_env["PERRY_GC_MOVING_LOOP_POLLS"] == "1"
            assert run_env["PERRY_GC_HEAP_LIMIT"] == "8"
        else:
            for effective in (compile_env, run_env):
                assert "PERRY_GC_SCHEDULE_SEED" not in effective, (cell["arm"], effective)
                assert "PERRY_GC_PROTECT_FROMSPACE" not in effective, (cell["arm"], effective)
        if cell["arm"] == "shipped_default":
            assert compile_env == {}
            assert run_env == {"PERRY_GC_TRACE": "1", "PERRY_GC_DIAG": "1"}
            assert cell["result"] == "PASS"
        else:
            assert cell["result"] == "UNVER", "command probes must never certify moving GC"
        if cell["arm"] == "wb_off":
            assert compile_env == {"PERRY_WRITE_BARRIERS": "0"}
            assert run_env["PERRY_WRITE_BARRIERS"] == "0"


def self_test() -> None:
    for fixture, seed in [("protected_poll_reclaim", "1"), ("ic_leaf_json", "12435")]:
        result = subprocess.run(
            [sys.executable, "-B", str(ROOT / "scripts/gc_matrix_fixture_env.py"),
             str(real_fixture(f"test_gap_gc_{fixture}"))],
            capture_output=True, text=True, check=True,
        )
        settings = assignments(result.stdout)
        assert settings["PERRY_GC_SCHEDULE_SEED"] == seed
        assert settings["PERRY_GC_SCHEDULE_RATE"] == "1"
        assert settings["PERRY_GC_SCHEDULE_ALLOC_KB"] == "0"
        assert settings["PERRY_GC_PROTECT_FROMSPACE"] == "1"
        if fixture == "ic_leaf_json":
            assert settings["PERRY_GC_VERIFY_EVACUATION"] == "1"
    matrix = (ROOT / "scripts/gc_repsel_matrix.sh").read_text()
    original_fixture = real_fixture(TEST).read_text()
    with tempfile.TemporaryDirectory(prefix="gc-matrix-routing-test-") as work:
        root = Path(work)
        for directory in ("scripts", "tests/fixtures/programs", "tests/parity", "probes", "target/release"):
            (root / directory).mkdir(parents=True)
        (root / "tests/parity/node-suite/http").mkdir(parents=True)
        shutil.copy(ROOT / "scripts/gc_matrix_fixture_env.py", root / "scripts")
        shutil.copy(ROOT / "scripts/parity_fixture_limits.py", root / "scripts")
        shutil.copy(ROOT / "scripts/gc_corpus_sources.py", root / "scripts")
        shutil.copy(ROOT / "scripts/parity.py", root / "scripts")
        shutil.copy(ROOT / "scripts/parity_normalize.py", root / "scripts")
        shutil.copy(ROOT / ".node-version", root)
        (root / f"tests/fixtures/programs/{TEST}.ts").write_text(original_fixture)
        (root / f"tests/parity/node-suite/http/{TEST}.ts").write_text(original_fixture)
        (root / "tests/parity/gc_repsel_corpus.txt").write_text(TEST + "\n")
        (root / "probes/node").write_text(
            "#!/usr/bin/env python3\nimport json, os, sys\nfrom pathlib import Path\n"
            "if '--version' not in sys.argv:\n"
            " source = next((arg for arg in reversed(sys.argv) if arg.endswith('.ts')), '')\n"
            " if not source or not Path(source).is_file(): raise SystemExit(3)\n"
            " with open(os.environ['ROUTING_RECEIPTS'], 'a') as f: "
            "f.write(json.dumps({'stage':'oracle','argv':sys.argv[1:], 'source':source})+'\\n')\n"
            "print('v' + Path('.node-version').read_text().strip() if '--version' in sys.argv else 'routing-probe-only')\n"
        )
        compiler = root / "target/release/perry"
        compiler.write_text(
            "#!/usr/bin/env python3\nimport json, os, sys\nfrom pathlib import Path\n"
            "captured = {k:v for k,v in os.environ.items() if k.startswith('PERRY_') and k != 'PERRY_BIN'}\n"
            "source = next((arg for arg in sys.argv if arg.endswith('.ts')), '')\n"
            "if not source or not Path(source).is_file(): raise SystemExit(3)\n"
            "receipt = {'stage':'compile', 'compile_env':captured, 'argv':sys.argv[1:], 'source':source}\n"
            "with open(os.environ['ROUTING_RECEIPTS'], 'a') as f: f.write(json.dumps(receipt)+'\\n')\n"
            "output = Path(sys.argv[sys.argv.index('-o')+1])\n"
            "runtime = '#!/usr/bin/env python3\\nimport json, os\\n'\n"
            "runtime += 'receipt = '+repr({'stage':'run','compile_env':captured})+'\\n'\n"
            "runtime += \"receipt['run_env'] = {k:v for k,v in os.environ.items() if k.startswith('PERRY_')}\\n\"\n"
            "runtime += \"import resource; receipt['limit_as'] = resource.getrlimit(resource.RLIMIT_AS)\\n\"\n"
            "runtime += \"with open(os.environ['ROUTING_RECEIPTS'], 'a') as f: f.write(json.dumps(receipt)+'\\\\n')\\n\"\n"
            "runtime += \"print('routing-probe-only')\\n\"\n"
            "output.write_text(runtime); output.chmod(0o755)\n"
        )
        compiler.chmod(0o755)
        (root / "probes/node").chmod(0o755)
        legacy = root / f"tests/fixtures/programs/{TEST}.ts"
        moved_source = f"tests/parity/node-suite/http/{TEST}.ts"
        legacy.unlink()
        moved = run_probe(root, matrix)
        assert moved.returncode == 0, moved.stdout[-2000:]
        moved_receipts = [json.loads(line) for line in (root / "receipts.jsonl").read_text().splitlines()]
        assert any(r["stage"] == "oracle" and r["source"] == moved_source
                   for r in moved_receipts), moved_receipts
        compiles = [r for r in moved_receipts if r["stage"] == "compile"]
        assert compiles and all(r["source"] == moved_source for r in compiles), compiles
        assert len([r for r in moved_receipts if r["stage"] == "oracle"]) == 1
        report = json.loads((root / "report.json").read_text())
        assert len(report["cells"]) == 4 and {c["test"] for c in report["cells"]} == {TEST}
        assert {c["arm"] for c in report["cells"]} == set(ARMS.split(",")), report["cells"]
        assert any(r["stage"] == "run" and r["compile_env"].get("PERRY_GC_SCHEDULE_SEED") == "10061"
                   for r in moved_receipts), moved_receipts
        (root / "tests/parity/gc_repsel_corpus.txt").write_text(TEST + "\n" + TEST + "\n")
        repeated = run_probe(root, matrix)
        assert repeated.returncode == 0, repeated.stdout[-2000:]
        repeated_report = json.loads((root / "report.json").read_text())
        assert len(repeated_report["cells"]) == 8
        assert [cell["test"] for cell in repeated_report["cells"]] == [TEST] * 8
        (root / "tests/parity/gc_repsel_corpus.txt").write_text(TEST + "\n")
        (root / moved_source).unlink()
        legacy.write_text(original_fixture)
        check_receipts(root, run_probe(root, matrix))
        # Bad IDs and duplicate basenames fail before receipts or replacing a prior report.
        report_path = root / "report.json"
        report_before = report_path.read_bytes()
        source_path = legacy
        source_before = hashlib.sha256(source_path.read_bytes()).digest()
        (root / "tests/parity/gc_repsel_corpus.txt").write_text("unknown_gc_case\n")
        unknown = run_probe(root, matrix)
        assert unknown.returncode == 3, unknown.stdout
        assert unknown.stdout.strip() == (
            f"manifest lists unknown_gc_case but tests/fixtures/programs/unknown_gc_case.ts does not exist"
        ), unknown.stdout
        assert not (root / "receipts.jsonl").exists()
        assert report_path.read_bytes() == report_before
        assert hashlib.sha256(source_path.read_bytes()).digest() == source_before
        (root / "tests/parity/gc_repsel_corpus.txt").write_text(TEST + "\n")
        (root / moved_source).write_text(original_fixture)
        ambiguous = run_probe(root, matrix)
        assert ambiguous.returncode == 3 and "ambiguous GC witness source" in ambiguous.stdout, ambiguous.stdout
        assert not (root / "receipts.jsonl").exists()
        assert report_path.read_bytes() == report_before
        assert hashlib.sha256(source_path.read_bytes()).digest() == source_before
        (root / moved_source).unlink()
        assert any(r["stage"] == "compile" and r["source"] == moved_source
                   for r in moved_receipts), moved_receipts
        check_receipts(root, run_probe(root, matrix))
        exposed = "// parity-node-argv: --expose-gc\n" + original_fixture
        check_receipts(root, run_probe(root, matrix, exposed))
        oracle = [json.loads(line) for line in (root / "receipts.jsonl").read_text().splitlines()
                  if json.loads(line)["stage"] == "oracle"]
        assert len(oracle) == 1 and "--expose-gc" in oracle[0]["argv"], oracle
        (root / f"tests/fixtures/programs/{TEST}.ts").write_text(original_fixture)
        mutations = {
            "missing compile-time instruments": (
                'effective_cenv="$effective_cenv ${FIXTURE_ENVS[$ti]}"', 'effective_cenv="$effective_cenv"'),
            "metadata leaks to OFF/control arms": (
                'if [ "$id" = loop_polls ] &&', 'if [ "$id" != absent ] &&'),
            "compile group aliases safepoint_minor": (
                'slug="${slug}_fixture"; fixture_group=1', 'fixture_group=1'),
        }
        for name, (old, new) in mutations.items():
            assert old in matrix
            result = run_probe(root, matrix.replace(old, new))
            try:
                check_receipts(root, result)
            except (AssertionError, KeyError):
                pass
            else:
                raise AssertionError(f"routing proof failed to reject sabotage: {name}")
        result = run_probe(root, matrix, "// parity-env: PERRY_NO_AUTO_OPTIMIZE=1\n" + original_fixture)
        assert result.returncode == 2, result.stdout
        assert not (root / "receipts.jsonl").exists(), "malformed metadata reached compiler"
        for name in ("fetch_streaming_chunks", "worker_transfer_ownership", "ic_leaf_json", "protected_poll_reclaim"):
            fixture = real_fixture(f"test_gap_gc_{name}")
            result = run_probe(root, matrix, fixture.read_text())
            report = json.loads((root / "report.json").read_text())
            receipts = [json.loads(line) for line in (root / "receipts.jsonl").read_text().splitlines()]
            oracle = next(r for r in receipts if r["stage"] == "oracle")
            assert ("--expose-gc" in oracle["argv"]) == (name in ("fetch_streaming_chunks", "worker_transfer_ownership"))
            expected = assignments(subprocess.check_output(
                [sys.executable, "-B", str(ROOT / "scripts/gc_matrix_fixture_env.py"), str(fixture)], text=True))
            for cell in report["cells"]:
                for field in ("compile_env", "run_env"):
                    delivered = assignments(cell[field])
                    for key, value in expected.items():
                        if cell["arm"] == "loop_polls":
                            assert delivered[key] == value, (name, field, key)
                        else:
                            assert key not in delivered, (name, cell["arm"], key)
                if cell["arm"] == "loop_polls" and "exit=125" in cell["evidence"]:
                    assert name == "protected_poll_reclaim" and cell["result"] == "FAIL"
                    assert not any(r["stage"] == "run" and
                                   r["compile_env"].get("PERRY_GC_SCHEDULE_SEED") == expected["PERRY_GC_SCHEDULE_SEED"]
                                   for r in receipts), "unsupported ceiling must not run witness"
                else:
                    assert cell["result"] == ("PASS" if cell["arm"] == "shipped_default" else "UNVER")
            if name == "protected_poll_reclaim":
                for receipt in (r for r in receipts if r["stage"] == "run"):
                    if "PERRY_GC_SCHEDULE_SEED" in receipt["compile_env"]:
                        assert all(0 < limit <= 2147483648 for limit in receipt["limit_as"])
    print("GC matrix routing self-test: PASS (actual command env, 3 sabotage controls, early refusal; no GC acceptance)")


if __name__ == "__main__":
    self_test()
