#!/usr/bin/env python3
"""Exercise matrix env dispatch with command probes, never GC acceptance.

Copies the original suffix fixture verbatim. The probes do not compile or run
TypeScript and emit no GC counters: moving cells must stay UNVER. Receipts
check the environment actually delivered to each compile/run command.
"""

from __future__ import annotations

import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parent.parent
TEST = "test_gap_gc_string_suffix_cursor"
ARMS = "loop_polls,safepoint_minor,shipped_default,wb_off"


def assignments(text: str) -> dict[str, str]:
    return dict(word.split("=", 1) for word in text.split())


def run_probe(root: Path, matrix: str, metadata: str | None = None) -> subprocess.CompletedProcess[str]:
    (root / "scripts/gc_repsel_matrix.sh").write_text(matrix)
    if metadata is not None:
        (root / f"test-files/{TEST}.ts").write_text(metadata)
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
    matrix = (ROOT / "scripts/gc_repsel_matrix.sh").read_text()
    original_fixture = (ROOT / f"test-files/{TEST}.ts").read_text()
    with tempfile.TemporaryDirectory(prefix="gc-matrix-routing-test-") as work:
        root = Path(work)
        for directory in ("scripts", "test-files", "test-parity", "probes", "target/release"):
            (root / directory).mkdir(parents=True)
        shutil.copy(ROOT / "scripts/gc_matrix_fixture_env.py", root / "scripts")
        shutil.copy(ROOT / ".node-version", root)
        (root / f"test-files/{TEST}.ts").write_text(original_fixture)
        (root / "test-parity/gc_repsel_corpus.txt").write_text(TEST + "\n")
        (root / "probes/node").write_text(
            "#!/usr/bin/env python3\nimport sys\nfrom pathlib import Path\n"
            "print('v' + Path('.node-version').read_text().strip() if '--version' in sys.argv else 'routing-probe-only')\n"
        )
        compiler = root / "target/release/perry"
        compiler.write_text(
            "#!/usr/bin/env python3\nimport json, os, sys\nfrom pathlib import Path\n"
            "captured = {k:v for k,v in os.environ.items() if k.startswith('PERRY_') and k != 'PERRY_BIN'}\n"
            "receipt = {'stage':'compile', 'compile_env':captured}\n"
            "with open(os.environ['ROUTING_RECEIPTS'], 'a') as f: f.write(json.dumps(receipt)+'\\n')\n"
            "output = Path(sys.argv[sys.argv.index('-o')+1])\n"
            "runtime = '#!/usr/bin/env python3\\nimport json, os\\n'\n"
            "runtime += 'receipt = '+repr({'stage':'run','compile_env':captured})+'\\n'\n"
            "runtime += \"receipt['run_env'] = {k:v for k,v in os.environ.items() if k.startswith('PERRY_')}\\n\"\n"
            "runtime += \"with open(os.environ['ROUTING_RECEIPTS'], 'a') as f: f.write(json.dumps(receipt)+'\\\\n')\\n\"\n"
            "runtime += \"print('routing-probe-only')\\n\"\n"
            "output.write_text(runtime); output.chmod(0o755)\n"
        )
        compiler.chmod(0o755)
        (root / "probes/node").chmod(0o755)
        check_receipts(root, run_probe(root, matrix))
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
    print("GC matrix routing self-test: PASS (actual command env, 3 sabotage controls, early refusal; no GC acceptance)")


if __name__ == "__main__":
    self_test()
