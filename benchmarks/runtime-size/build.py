#!/usr/bin/env python3
"""Build an isolated runtime and the #13662 scan without editing source files."""
import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
from pathlib import Path


HERE = Path(__file__).resolve().parent
MINIMAL = "perry-runtime/keepalive-anchors,perry-stdlib/async-bridge"


def lto_rules(source):
    # Fat LTO can fold these forwarding wrappers into their known callees.
    # ADD the callees as alternatives; retain the wrappers and callback edges.
    rules = (source / "scripts/gc_call_effects/seeds.txt").read_text()
    for wrapper, targets in (
        ("js_async_hooks_provider_defer_destroy",
         "provider_ffi::(js_async_hooks_provider_defer_destroy|defer_destroy_after_check_turns)"),
        ("js_async_hooks_provider_destroy",
         "(provider_ffi::js_async_hooks_provider_destroy|destroy)"),
    ):
        before = (f"indirect exact {wrapper} => exact "
                  f"perry_runtime::async_hooks::provider_ffi::{wrapper} --")
        after = (f"indirect exact {wrapper} => regex "
                 f"^perry_runtime::async_hooks::{targets}$ --")
        if rules.count(before) != 1:
            raise ValueError(f"callback rule changed; review LTO compatibility: {wrapper}")
        rules = rules.replace(before, after)
    return rules


def sidecars(source, runtime, effects):
    rules = runtime / "seeds.txt"
    rules.write_text(lto_rules(source))
    command = [str(effects), "--root", str(source), "--rules", str(rules),
               "sidecars", str(runtime)]
    result = subprocess.run(command, capture_output=True, text=True)
    log = result.stdout + result.stderr
    (runtime / "effects-initial.log").write_text(log)
    if result.returncode:
        # Omitting an absent teardown CUT is conservative. Never remove a GC
        # seed, suppress an unknown call, or bypass the archive fingerprint.
        expected = "teardown ^<perry_runtime::arena::block::Arena as core::ops::drop::Drop>::drop$"
        unmatched = re.findall(r"^  seeds\.txt:\d+: (.+)$", log, re.MULTILINE)
        if "named seed/cut rules that match NO node:" not in log or unmatched != [expected]:
            raise RuntimeError(f"unexpected effects failure; inspect {runtime / 'effects-initial.log'}")
        text = rules.read_text()
        cuts = [line for line in text.splitlines(keepends=True)
                if line.startswith("teardown regex " + expected.removeprefix("teardown "))]
        if len(cuts) != 1:
            raise ValueError("expected exactly one Arena::drop teardown cut")
        rules.write_text(text.replace(cuts[0], "# Fat LTO eliminated this teardown body.\n"))
        result = subprocess.run(command, capture_output=True, text=True)
        (runtime / "effects-final.log").write_text(result.stdout + result.stderr)
        result.check_returncode()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--compiler", type=Path, required=True)
    parser.add_argument("--effects", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--profile", choices=("o2", "oz"), default="o2")
    parser.add_argument("--features", default=MINIMAL)
    parser.add_argument("--jobs", type=int, default=2)
    args = parser.parse_args()
    if "RUSTFLAGS" in os.environ or "CARGO_ENCODED_RUSTFLAGS" in os.environ:
        parser.error("unset Rust flag overrides so the repository's unwind configuration applies")
    source = args.source.resolve(strict=True)
    compiler = args.compiler.resolve(strict=True)
    effects = args.effects.resolve(strict=True)
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    runtime = output / "runtime"
    runtime.mkdir(exist_ok=True)
    profile = "issue-" + args.profile
    config = HERE / (args.profile + ".toml")
    # Run from the source root so its pinned toolchain and .cargo/config.toml
    # (including synchronous unwind tables) apply. Do not override RUSTFLAGS.
    command = ["cargo", "--config", str(config), "build", "--locked",
               "--target-dir", str(output / "target"), "--profile", profile,
               "-j", str(args.jobs), "-p", "perry-runtime-static", "-p", "perry-stdlib-static",
               "--no-default-features", "--features", args.features]
    subprocess.run(command, cwd=source, check=True)
    archives = ("libperry_runtime.a", "libperry_stdlib.a")
    for name in archives:
        shutil.copyfile(output / "target" / profile / name, runtime / name)
    sidecars(source, runtime, effects)
    env = dict(os.environ, PERRY_RUNTIME_DIR=str(runtime), PERRY_NO_AUTO_OPTIMIZE="1",
               PERRY_NO_CACHE="1", PERRY_DISABLE_BUILD_CACHE="1", PERRY_KEEP_SYMBOLS="1",
               PERRY_LL_OPT_LEVEL="2", PERRY_SPECIALIZED_ABI="1")
    subprocess.run([str(compiler), "compile", str(HERE / "scan.ts"), "--no-cache",
                    "-o", str(output / "scan"), "--strict-eval", "--strict-dynamic-import",
                    "--strict-unimplemented"], cwd=source, env=env, check=True)
    shutil.copyfile(output / "scan", output / "scan.symbols")
    subprocess.run(["strip", str(output / "scan")], check=True)
    if os.uname().sysname == "Darwin":
        subprocess.run(["codesign", "--force", "--sign", "-", str(output / "scan")], check=True)
    provenance = {
        "commit": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=source, text=True).strip(),
        "diff": subprocess.check_output(["git", "diff", "HEAD", "--", "crates/",
                                         "Cargo.toml", "Cargo.lock", ".cargo/"], cwd=source, text=True),
        "profile": config.read_text(), "features": args.features,
        "rustflags": os.environ.get("RUSTFLAGS"),
        "rustc": subprocess.check_output(["rustc", "-Vv"], cwd=source, text=True),
        "compiler_sha256": hashlib.sha256(compiler.read_bytes()).hexdigest(),
        "archive_sha256": {name: hashlib.sha256((runtime / name).read_bytes()).hexdigest()
                           for name in archives},
    }
    (output / "provenance.json").write_text(json.dumps(provenance, indent=2) + "\n")


if __name__ == "__main__":
    main()
