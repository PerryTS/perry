#!/usr/bin/env python3
"""Compare the Windows runtime regressions with the repository's Node oracle."""
import argparse
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
from windows_pipe_drain_cancel import check as check_drain_cancel

ROOT = Path(__file__).resolve().parents[1]


def run(command, env, timeout):
    process = subprocess.Popen(command, cwd=ROOT, env=env, stdout=subprocess.PIPE,
                               stderr=subprocess.PIPE, text=True, encoding="utf-8")
    try:
        stdout, stderr = process.communicate(timeout=timeout)
    except subprocess.TimeoutExpired:
        subprocess.run(["taskkill", "/PID", str(process.pid), "/T", "/F"],
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=False)
        process.communicate()
        raise RuntimeError(f"Timed out: {command}") from None
    return process.returncode, stdout.replace("\r\n", "\n"), stderr


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--compiler", type=Path, default=ROOT / "target/debug/perry.exe")
    parser.add_argument("--runtime-dir", type=Path, default=ROOT / "target/debug")
    args = parser.parse_args()
    if os.name != "nt":
        parser.error("These regressions require a native Windows host")
    node = shutil.which("node")
    if not node:
        parser.error("Node is required")
    pin = (ROOT / ".node-version").read_text(encoding="utf-8").strip()
    version = subprocess.check_output([node, "--version"], text=True).strip().removeprefix("v")
    if version != pin:
        parser.error(f"Node oracle must match .node-version ({pin}); found {version}")
    env = os.environ.copy()
    env["PERRY_RUNTIME_DIR"] = str(args.runtime_dir.resolve())
    fixtures = sorted((ROOT / "test-files").glob("test_gap_windows_*.ts"))
    if not fixtures:
        raise RuntimeError("No Windows regression fixtures found")
    with tempfile.TemporaryDirectory(prefix="perry-windows-regressions-") as directory:
        for fixture in fixtures:
            exe = str(Path(directory) / (fixture.stem + ".exe"))
            oracle = run([node, str(fixture)], env, 30)
            compiled = run([str(args.compiler.resolve()), "compile", str(fixture),
                            "--no-auto-optimize", "-o", exe], env, 120)
            if compiled[0] != 0:
                raise RuntimeError(f"Compile failed for {fixture.name}:\n{compiled[1]}{compiled[2]}")
            actual = run([exe], env, 30)
            if oracle[0] != 0 or actual[:2] != oracle[:2]:
                raise RuntimeError(f"{fixture.name}\nNode: {oracle}\nPerry: {actual}")
            Path(exe).unlink()
            print(f"PASS {fixture.name}", flush=True)
        check_drain_cancel(args.compiler.resolve(), env, directory)
    print(f"{len(fixtures)} Windows regressions match Node {pin}")


if __name__ == "__main__":
    main()
