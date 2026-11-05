#!/usr/bin/env python3
"""Check Node/Perry executable identity through four invocation spellings."""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
PERRY = Path(os.environ.get("PERRY_BIN", ROOT / "target/release/perry")).resolve()
NODE = shutil.which(os.environ.get("NODE_BIN", "node"))
if not NODE:
    raise SystemExit("Node is required")
SOURCES = [(ROOT / "test-files/test_gap_process_execpath_absolute.ts", 11, []),
           (ROOT / "test-files/test_gap_process_execpath_cluster.ts", 2, []),
           (ROOT / "test-files/test_gap_process_execpath_cluster.ts", 2,
            ["--cluster-user-arg"])]


def output(command, cwd, env):
    return subprocess.run(command, cwd=cwd, env=env, check=True,
                          text=True, capture_output=True, timeout=120).stdout


with tempfile.TemporaryDirectory(prefix="perry-execpath-") as tmp:
    directory = Path(tmp)
    binary = directory / "main_bin"
    (directory / "perry-link").symlink_to(binary)
    (directory / "node-link").symlink_to(NODE)
    perry_names = ["./main_bin", str(binary), "./perry-link", "main_bin"]
    node_names = ["./node-link", str(directory / "node-link"),
                  "./node-link", "node-link"]
    for source, lines, args in SOURCES:
        subprocess.run([str(PERRY), "compile", str(source), "-o", str(binary)],
                       check=True, timeout=300)
        for mode, perry_name, node_name in zip(
                ["relative", "absolute", "symlink", "PATH"], perry_names, node_names):
            env = dict(os.environ, PATH=tmp + os.pathsep + os.environ.get("PATH", ""))
            if args:
                env["PERRY_TEST_CLUSTER_ARG"] = args[0]
            else:
                env.pop("PERRY_TEST_CLUSTER_ARG", None)
            env["PERRY_TEST_EXEC_ARGV0"] = node_name
            env["PERRY_TEST_EXEC_PATH"] = str(Path(NODE).resolve())
            oracle = output([node_name, str(source), *args], directory, env)
            env["PERRY_TEST_EXEC_ARGV0"] = perry_name
            env["PERRY_TEST_EXEC_PATH"] = str(binary.resolve())
            actual = output([perry_name, *args], directory, env)
            assert oracle == "true\n" * lines, (source.name, mode, "Node", oracle)
            assert actual == oracle, (source.name, mode, "Perry", actual, "Node", oracle)
            print(source.name + (" args " if args else " ") + mode + ": PASS")
