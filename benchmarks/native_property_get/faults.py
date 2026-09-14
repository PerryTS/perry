#!/usr/bin/env python3
"""Run three source mutations serially, restoring each file even on failure.

Run only while no other build or validation reads this worktree.
"""
import argparse
import json
from pathlib import Path
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", required=True, type=Path)
    args = parser.parse_args()
    repo = Path(__file__).resolve().parents[2]
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=False)
    native_get = repo / "crates/perry-runtime/src/object/native_get.rs"
    expando = repo / "crates/perry-runtime/src/object/exotic_expando.rs"
    prefix = "object::native_get::tests::"
    mutations = [
        ("fast_path_removed", native_get,
         "if !receiver.is_pointer() || key.first() == Some(&b'#')",
         "if true || !receiver.is_pointer() || key.first() == Some(&b'#')",
         prefix + "own_data_is_served_and_matches_forced_slow_for_all_value_kinds"),
        ("accessor_guard_removed", native_get,
         "if meta.is_null() || (*meta).accessor_key_bits & accessor_bit == 0 {",
         "if true {",
         prefix + "accessor_bloom_guard_preserves_own_and_inherited_getter_calls"),
        ("exotic_expando_ignored", expando,
         'if kind != ExoticKind::Error && !expando_in_use() && !super::descriptors_in_use() {',
         'if kind == ExoticKind::RegExp && name == "value" { return None; }\n'
         '    if kind != ExoticKind::Error && !expando_in_use() && !super::descriptors_in_use() {',
         prefix + "regexp_expandos_and_accessors_remain_on_the_exotic_path"),
    ]

    def test(name, label, exact=False):
        argv = ["cargo", "test", "-j4", "-p", "perry-runtime", "--lib", name,
                "--", "--test-threads=1"]
        if exact:
            argv.append("--exact")
        with (out / (label + ".log")).open("w") as log:
            result = subprocess.run(argv, cwd=repo, stdout=log, stderr=subprocess.STDOUT)
        return result.returncode, (out / (label + ".log")).read_text()

    code, _ = test("object::native_get::tests", "healthy_before")
    if code:
        raise RuntimeError("healthy native Get tests failed before fault injection")
    results = []
    for name, path, old, new, test_name in mutations:
        original = path.read_bytes()
        text = original.decode()
        if text.count(old) != 1:
            raise ValueError(f"{name}: mutation anchor is not unique")
        try:
            path.write_text(text.replace(old, new))
            code, log = test(test_name, name, exact=True)
            killed = code != 0 and f"test {test_name} ... FAILED" in log and "test result: FAILED" in log
            results.append({"mutation": name, "test": test_name,
                            "exit_code": code, "assertion_failed": killed})
            (out / "results.json").write_text(json.dumps(results, indent=2) + "\n")
            if not killed:
                raise RuntimeError(f"{name}: expected an assertion failure, see log")
            print(name, "killed by", test_name, flush=True)
        finally:
            path.write_bytes(original)
    code, _ = test("object::native_get::tests", "healthy_after")
    if code:
        raise RuntimeError("healthy native Get tests failed after restoring sources")


if __name__ == "__main__":
    main()
