#!/usr/bin/env python3
"""PERRY_FACTS_DCE: an inactive body is a trap; on by default (`PERRY_FACTS_DCE=0` turns it off);
incomplete reach removes nothing; a false inactive claim traps, and check mode reports it first."""
import argparse
import os
from pathlib import Path
import subprocess


def run(args, **kw):
    return subprocess.run(args, timeout=300, capture_output=True, **kw)


def main():
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument("--perry", type=Path, required=True)
    parser.add_argument("--node", default="node")
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    args.perry = args.perry.resolve()
    args.out = args.out.resolve()
    args.out.mkdir(parents=True, exist_ok=True)
    fixtures = Path(__file__).with_name("dce_stub").resolve()
    base = dict(os.environ, PERRY_FACTS_UNITS="0", PERRY_NO_CACHE="1", PERRY_DISABLE_BUILD_CACHE="1",
                PERRY_NO_LINK_CACHE="1", PERRY_NO_ARCHIVE_CACHE="1", PERRY_ALLOW_PERRY_FEATURES="1")
    for k in ("PERRY_FACTS_DCE", "PERRY_VERIFY_FACTS", "PERRY_FACTS_SABOTAGE", "PERRY_FACTS_MAX_WORK"):
        base.pop(k, None)

    def build(source, name, **overrides):
        exe = args.out / name
        ll = args.out / (name + ".ll")
        ll.mkdir(exist_ok=True)
        for old in ll.glob("*.ll"):
            old.unlink()
        env = dict(base, PERRY_SAVE_LL=str(ll), **overrides)
        built = run([str(args.perry), "compile", "--cache-dir", str(args.out / "cache"),
                     str(source), "-o", str(exe)], env=env)
        assert built.returncode == 0, (name, built.stderr)
        traps = sum(p.read_text().count("call void @llvm.trap()") for p in ll.glob("*.ll"))
        return exe, env, traps

    def oracle(source):
        value = run([args.node, "--experimental-strip-types", str(source)])
        assert value.returncode == 0, value.stderr
        return value.stdout

    dead = fixtures / "dead.ts"
    want = oracle(dead)
    # Default (no switch): the dead bodies are traps and the program's output is unchanged.
    exe, env, traps = build(dead, "dead-default")
    assert traps >= 3, traps
    seen = run([str(exe)], env=env)
    assert seen.returncode == 0 and seen.stdout == want, seen
    # PERRY_FACTS_DCE=1 is the default spelled out.
    exe, env, traps = build(dead, "dead-on", PERRY_FACTS_DCE="1")
    assert traps >= 3, traps
    # PERRY_FACTS_DCE=0: no trap is emitted, output is Node's.
    exe, env, traps = build(dead, "dead-off", PERRY_FACTS_DCE="0")
    assert traps == 0, traps
    assert run([str(exe)], env=env).stdout == want
    # An incomplete reach (the solve is stopped by its work bound) removes nothing.
    stopped = args.out / "stopped.ts"
    stopped.write_text(dead.read_text() + "function f(x: any) { return x.k; }\n" + "console.log(f({k: 1}));\n" * 256)
    want_stopped = oracle(stopped)
    exe, env, traps = build(stopped, "stopped-complete")
    assert traps >= 3, traps
    exe, env, traps = build(stopped, "stopped-incomplete", PERRY_FACTS_MAX_WORK="1")
    assert traps == 0, traps
    seen = run([str(exe)], env=env)
    assert seen.returncode == 0 and seen.stdout == want_stopped, seen

    # Once a false inactive claim (CXCHK0 findings, accepted witness
    # language/array/test_gap_10086): the iterator's `return` that
    # destructuring runs is reached now, so the default build keeps it.
    closing = fixtures / "false-inactive.ts"
    want = oracle(closing)
    exe, env, traps = build(closing, "closing-default")
    seen = run([str(exe)], env=env)
    assert seen.returncode == 0 and seen.stdout == want, seen

    # A false inactive claim made on purpose (S0 control `noreachprotoaccessors`:
    # a getter on String.prototype read on a string is called inactive).
    wrong = Path(__file__).with_name("verify_facts_s0").resolve() / "reach-proto-accessor-negative.ts"
    want = oracle(wrong)
    exe, env, traps = build(wrong, "wrong-correct")
    assert run([str(exe)], env=env).stdout == want
    # Default: the wrong inactive claim is executed as a trap (SIGILL / SIGTRAP).
    exe, env, traps = build(wrong, "wrong-default", PERRY_FACTS_SABOTAGE="noreachprotoaccessors")
    assert traps >= 1, traps
    seen = run([str(exe)], env=env)
    assert seen.returncode != 0 and seen.stdout != want, seen
    # PERRY_FACTS_DCE=0 keeps the body: output is Node's despite the wrong claim.
    exe, env, traps = build(wrong, "wrong-off", PERRY_FACTS_SABOTAGE="noreachprotoaccessors", PERRY_FACTS_DCE="0")
    assert traps == 0, traps
    assert run([str(exe)], env=env).stdout == want
    # With the check mode the report takes precedence over the trap.
    exe, env, traps = build(wrong, "wrong-verify", PERRY_FACTS_SABOTAGE="noreachprotoaccessors", PERRY_VERIFY_FACTS="1")
    seen = run([str(exe)], env=env)
    assert seen.returncode == 86 and b"PERRY_FALSE_PROOF" in seen.stderr, seen
    print("dce stub: ok")


if __name__ == "__main__":
    main()
