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
    def inherits_development(name):
        seen = set()
        current = name
        while current in profiles and current not in seen:
            seen.add(current)
            if current in ("dev", "perry-dev"):
                return True
            parent = profiles[current].get("inherits")
            if not isinstance(parent, str):
                return False
            current = parent
        return False

    development_profiles = {
        name for name in profiles if inherits_development(name)
    }
    for name, profile in profiles.items():
        if name == "test" or name in development_profiles:
            if "debug" in profile and profile["debug"] != "line-tables-only":
                errors.append(f"profile.{name} overrides development line tables")

    for name in development_profiles | {"test"}:
        profile = profiles.get(name, {})
        build_override = profile.get("build-override", {})
        if "debug" in build_override and build_override["debug"] != "line-tables-only":
            errors.append(f"profile.{name}.build-override.debug overrides line tables")
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
        bad = copy.deepcopy(data)
        bad["profile"]["dev"].setdefault("build-override", {})["debug"] = 2
        assert check(bad)
        bad = copy.deepcopy(data)
        bad["profile"]["local"] = {"inherits": "dev"}
        bad["profile"]["local"].setdefault("package", {})["fixture"] = {"debug": 2}
        assert check(bad)
        bad = copy.deepcopy(data)
        bad["profile"]["dev-derived-middle"] = {"inherits": "dev"}
        bad["profile"]["dev-derived-leaf"] = {"inherits": "dev-derived-middle", "debug": 2}
        assert check(bad)
        bad = copy.deepcopy(data)
        bad["profile"]["dev-derived-middle"] = {"inherits": "dev"}
        bad["profile"]["dev-derived-leaf"] = {
            "inherits": "dev-derived-middle",
            "build-override": {"debug": 2},
        }
        assert check(bad)
        print("PASS: development debug policy rejects full-debug regressions")
        return
    errors = check(data)
    if errors:
        parser.exit(1, "\n".join(errors) + "\n")
    print("PASS: development profiles retain line tables")


if __name__ == "__main__":
    main()
