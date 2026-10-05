#!/usr/bin/env python3
"""Enforce line-table debug info for developer builds without changing shipping profiles."""
import argparse
from pathlib import Path
import tomllib


def check(data):
    profiles = data["profile"]
    errors = []
    for name in ("dev", "perry-dev"):
        if profiles[name].get("debug") != "line-tables-only":
            errors.append(f"profile.{name}.debug must be line-tables-only")
    for name, profile in profiles.items():
        if name == "test" or profile.get("inherits") in ("dev", "perry-dev"):
            if "debug" in profile and profile["debug"] != "line-tables-only":
                errors.append(f"profile.{name} overrides development line tables")
    for name in ("dev", "test", "perry-dev"):
        for package, override in profiles.get(name, {}).get("package", {}).items():
            if "debug" in override and override["debug"] != "line-tables-only":
                errors.append(f"profile.{name}.package.{package} overrides line tables")
    return errors


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    data = tomllib.loads((Path(__file__).resolve().parents[1] / "Cargo.toml").read_text())
    if args.self_test:
        import copy
        assert not check(data)
        for name in ("dev", "perry-dev"):
            bad = copy.deepcopy(data)
            bad["profile"][name]["debug"] = True
            assert check(bad)
        bad = copy.deepcopy(data)
        bad["profile"]["test"] = {"debug": 2}
        assert check(bad)
        bad = copy.deepcopy(data)
        bad["profile"]["dev"].setdefault("package", {})["fixture"] = {"debug": 2}
        assert check(bad)
        print("PASS: development debug policy rejects full-debug regressions")
        return
    errors = check(data)
    if errors:
        parser.exit(1, "\n".join(errors) + "\n")
    print("PASS: development profiles retain line tables")


if __name__ == "__main__":
    main()
