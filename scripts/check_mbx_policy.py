#!/usr/bin/env python3
"""Check the repository-owned MBX linker policy and release build entry points."""
import argparse
import re
from pathlib import Path
import tomllib

ROOT = Path(__file__).resolve().parents[1]
PROFILES = ("dev", "perry-dev", "release", "dist", "prod", "test", "bench", "gcaudit")
TARGETS = ("x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu")
SCRIPTS = ("build_core_runtime.sh", "build_linux_glibc_2_31.sh", "build_linux_musl.sh")


def check(policy, scripts):
    errors = []
    if policy.get("share_workspace_root") is not False:
        errors.append("Keep real source paths: share_workspace_root must be false")
    linker = policy.get("linker", {})
    if linker.get("default") != "system":
        errors.append("Non-GNU targets must retain their system linker")
    for profile in PROFILES:
        table = linker.get("profiles", {}).get(profile, {})
        if table != {target: "mold@2.42.0" for target in TARGETS}:
            errors.append(f"{profile}: pin mold 2.42.0 only for Linux GNU targets")
    for name, script in scripts.items():
        active_lines = [
            line for line in script.splitlines()
            if line.strip() and not line.lstrip().startswith("#")
        ]
        active_script = "\n".join(active_lines)
        assignments = re.findall(r"^\s*cargo_build\s*=\s*\(([^)]*)\)", active_script, re.M)
        if not any(re.fullmatch(r"\s*mbx\s+build\s*", command) for command in assignments):
            errors.append(f"{name}: cargo_build must select mbx build")
        if any(re.match(r"\s*cargo(?:\s|$)", command) for command in assignments):
            errors.append(f"{name}: cargo_build must not select Cargo")
        if re.search(r"(?m)^\s*cargo\s+build(?:\s|$)", active_script):
            errors.append(f"{name}: direct cargo build bypasses MBX")
        if not re.search(r"(?m)^\s*if\s+!\s+command\s+-v\s+mbx\s*>/dev/null", active_script):
            errors.append(f"{name}: builds must use MBX without a silent Cargo fallback")
    return errors


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    try:
        policy = tomllib.loads((ROOT / ".mbx.toml").read_text())
        scripts = {name: (ROOT / "scripts" / name).read_text() for name in SCRIPTS}
    except (OSError, tomllib.TOMLDecodeError) as exc:
        parser.exit(1, f"check_mbx_policy: could not load policy inputs: {exc}\n")
    if args.self_test:
        import copy
        assert not check(policy, scripts)
        for profile in PROFILES:
            bad = copy.deepcopy(policy)
            bad["linker"]["profiles"][profile][TARGETS[0]] = "system"
            assert check(bad, scripts)
        bad = copy.deepcopy(policy)
        bad["linker"]["default"] = "mold@2.42.0"
        assert check(bad, scripts)
        for name in SCRIPTS:
            bad = dict(scripts)
            bad[name] += "\ncargo_build=(cargo build)\n"
            assert check(policy, bad)
            for fallback in ("cargo_build=(cargo build --locked)", "cargo build --release"):
                bad = dict(scripts)
                bad[name] += f"\n{fallback}\n"
                assert check(policy, bad)
            bad = dict(scripts)
            bad[name] = bad[name].replace(
                "command -v mbx >/dev/null 2>&1",
                "# command -v mbx >/dev/null 2>&1",
            )
            assert check(policy, bad)
        print("PASS: linker and uncached-fallback regressions rejected")
        return
    errors = check(policy, scripts)
    if errors:
        parser.exit(1, "\n".join(errors) + "\n")
    print("PASS: MBX policy")


if __name__ == "__main__":
    main()
