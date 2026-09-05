#!/usr/bin/env python3
"""Pinned Claude Code native parity gate. Runtime checks require macOS Seatbelt."""

import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tarfile
import tempfile
import time
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
CORPUS = ROOT / "tests/cc-parity"
SANDBOX = ["/usr/bin/sandbox-exec", "-p", "(version 1) (allow default) (deny network*)"]
CASES = ("help", "version")


def digest(data):
    return hashlib.sha256(data).hexdigest()


def verify(data, expected, description):
    if len(data) != expected["bytes"] or digest(data) != expected["sha256"]:
        raise ValueError(f"{description}: size/SHA-256 mismatch")


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2) + "\n")


def prepare(work, manifest, archive_path=None):
    if archive_path:
        archive = archive_path.read_bytes()
    else:
        with urllib.request.urlopen(manifest["archive"]["url"], timeout=120) as response:
            archive = response.read()
    verify(archive, manifest["archive"], "npm archive")
    # Never extract paths, symlinks, or executable package hooks from the archive.
    with tarfile.open(fileobj=io.BytesIO(archive), mode="r:gz") as package:
        member = package.getmember("package/cli.js")
        if not member.isfile():
            raise ValueError("package/cli.js is not a regular file")
        with package.extractfile(member) as source:
            bundle = source.read()
    verify(bundle, manifest["bundle"], "cli.js")
    (work / "cli.js").write_bytes(bundle)
    write_json(work / "logs/source.json", manifest)


def build_toolchain(work):
    metadata = json.loads(subprocess.check_output(
        ["cargo", "metadata", "--no-deps", "--format-version", "1"], cwd=ROOT
    ))
    base = ["cargo", "build", "--locked", "--profile", "perry-dev"]
    # The compiler uses the default runtime without external Wasm symbols.
    commands = [base + ["-p", "perry"]]
    packages = ["perry-runtime-static", "perry-stdlib-static", "perry-wasm-host"]
    packages += sorted(p["name"] for p in metadata["packages"] if p["name"].startswith("perry-ext-"))
    # Unify wasm-host in EVERY archive embedding runtime code (#6303).
    runtime = base + ["--features", "perry-runtime/wasm-host"]
    for package in packages:
        runtime += ["-p", package]
    commands.append(runtime)
    with (work / "logs/build.log").open("w") as log:
        for command in commands:
            print(" ".join(command), flush=True)
            subprocess.run(command, cwd=ROOT, stdout=log, stderr=subprocess.STDOUT, check=True)


def run_logged(command, cwd, env, stdout, stderr, timeout):
    started = time.monotonic()
    with stdout.open("wb") as out, stderr.open("wb") as err:
        process = subprocess.Popen(
            command, cwd=cwd, env=env, stdout=out, stderr=err, start_new_session=True
        )
        timed_out = False
        try:
            process.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            timed_out = True
            os.killpg(process.pid, signal.SIGKILL)
            process.wait()
    return {
        "exit_code": process.returncode,
        "timed_out": timed_out,
        "seconds": round(time.monotonic() - started, 3),
        "stdout_bytes": stdout.stat().st_size,
        "stdout_sha256": digest(stdout.read_bytes()),
    }


def compile_bundle(work, manifest, perry, timeout):
    verify((work / "cli.js").read_bytes(), manifest["bundle"], "cli.js")
    binary = work / "claude-native"
    binary.unlink(missing_ok=True)  # A failed rebuild must never reuse an old executable.
    env = {key: value for key, value in os.environ.items() if not key.startswith("PERRY_")}
    env.update(PERRY_RUNTIME_DIR=str(perry.parent), PERRY_NO_AUTO_OPTIMIZE="1", PERRY_NO_CACHE="1",
               PERRY_CODEGEN_UNIT_JOBS="4")
    command = [str(perry), "compile", "--no-auto-optimize", "--no-cache",
               "--enable-wasm-runtime", str(work / "cli.js"), "-o", str(binary)]
    result = run_logged(command, work, env, work / "logs/compile.stdout",
                        work / "logs/compile.stderr", timeout)
    write_json(work / "logs/compile.json", {"command": command, **result})
    if result["exit_code"] != 0 or result["timed_out"]:
        raise ValueError("native compilation failed; see logs/compile.stderr and compile.json")
    require_native(binary)


def require_native(binary):
    # This gate runs on macOS; a Node wrapper must not satisfy the native arm.
    with binary.open("rb") as executable:
        magic = executable.read(4)
    if magic not in (b"\xcf\xfa\xed\xfe", b"\xfe\xed\xfa\xcf"):
        raise ValueError(f"{binary}: expected a 64-bit Mach-O executable")
    if not os.access(binary, os.X_OK):
        raise ValueError(f"{binary}: not executable")


def scratch_env(directory):
    # Do not inherit credentials, user configuration, or caller's PERRY_* knobs.
    return {
        "PATH": "/usr/bin:/bin:/usr/sbin:/sbin",
        "HOME": str(directory),
        "XDG_CONFIG_HOME": str(directory / "config"),
        "XDG_CACHE_HOME": str(directory / "cache"),
        "XDG_STATE_HOME": str(directory / "state"),
        "TMPDIR": str(directory),
        "LANG": "C.UTF-8", "LC_ALL": "C.UTF-8", "TERM": "dumb",
        "CI": "1", "NO_COLOR": "1", "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC": "1",
    }


def check(work, manifest, corpus=CORPUS, timeout=60, node=None):
    if sys.platform != "darwin" or not Path(SANDBOX[0]).is_file():
        raise ValueError("offline execution requires macOS sandbox-exec; no unsandboxed fallback")
    if node:
        verify((work / "cli.js").read_bytes(), manifest["bundle"], "cli.js")
        command = [str(node), str(work / "cli.js")]
        prefix = "node-"
    else:
        binary = work / "claude-native"
        require_native(binary)
        command = [str(binary)]
        prefix = ""
    report = {"bundle_sha256": manifest["bundle"]["sha256"], "cases": {}}
    for case in CASES:
        expected = (corpus / f"{case}.stdout").read_bytes()
        verify(expected, manifest["goldens"][case], f"{case} golden")
        stdout = work / f"logs/{prefix}{case}.stdout"
        stderr = work / f"logs/{prefix}{case}.stderr"
        with tempfile.TemporaryDirectory(prefix=f"cc-parity-{case}-") as scratch:
            directory = Path(scratch)
            result = run_logged(SANDBOX + command + [f"--{case}"], directory,
                                scratch_env(directory), stdout, stderr, timeout)
        result["matches_golden"] = stdout.read_bytes() == expected
        result["passed"] = (result["exit_code"] == 0 and not result["timed_out"]
                            and result["matches_golden"])
        report["cases"][case] = result
        print(f"{case}: {'PASS' if result['passed'] else 'FAIL'} {json.dumps(result)}", flush=True)
    write_json(work / f"logs/{prefix}parity.json", report)
    if not all(result["passed"] for result in report["cases"].values()):
        raise ValueError("Claude Code parity failed; compare logs/*.stdout with tests/cc-parity/*.stdout")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("prepare", "build", "compile", "check"))
    parser.add_argument("--work-dir", type=Path, required=True)
    parser.add_argument("--archive", type=Path, help="use a local pinned npm archive during prepare")
    parser.add_argument("--perry", type=Path, help="fresh compiler next to coherently built runtime archives")
    parser.add_argument("--node", type=Path, help="check the Node oracle locally instead of the native executable")
    parser.add_argument("--timeout", type=float, help="seconds; default compile 3600, check 60")
    args = parser.parse_args()
    work = args.work_dir.resolve()
    (work / "logs").mkdir(parents=True, exist_ok=True)
    manifest = json.loads((CORPUS / "manifest.json").read_text())
    try:
        if args.command == "prepare":
            prepare(work, manifest, args.archive)
        elif args.command == "build":
            build_toolchain(work)
        elif args.command == "compile":
            if args.perry is None:
                parser.error("compile requires --perry")
            compile_bundle(work, manifest, args.perry.resolve(), args.timeout or 3600)
        else:
            check(work, manifest, timeout=args.timeout or 60,
                  node=args.node.resolve() if args.node else None)
    except (OSError, ValueError, tarfile.TarError, subprocess.CalledProcessError) as error:
        print(f"cc-parity: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
