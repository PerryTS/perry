#!/usr/bin/env python3
"""Check mode over the parity corpora: every program of tests/parity/<corpus>/*/*.ts is compiled
with `PERRY_VERIFY_FACTS=1` and run; a false proof of the whole-program facts ends a run with
exit code 86 (or prints a PERRY_FALSE_PROOF line). The default corpora are `language` and
`node-suite`: node-suite reaches builtin models (Buffer, Object.assign on a primitive) that
language does not.

Exit code 1 when a program reports a false proof. A compile failure or another run failure is
counted and listed, not judged: this gate is about the proofs only."""
import argparse
from concurrent.futures import ThreadPoolExecutor
import os
from pathlib import Path
import subprocess
import sys

FALSE_PROOF_RC = 86
MARK = b"PERRY_FALSE_PROOF"


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--perry", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--corpus", action="append", help="directory under tests/parity (repeatable)")
    parser.add_argument("--jobs", type=int, default=max(1, (os.cpu_count() or 2) // 2))
    parser.add_argument("--compile-timeout", type=int, default=300)
    parser.add_argument("--run-timeout", type=int, default=60)
    parser.add_argument("--keep", action="store_true", help="keep the logs of clean programs too")
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parent.parent,
                        help="the checkout whose tests/parity is read (default: this script's)")
    args = parser.parse_args()
    args.perry = args.perry.resolve()
    args.out = args.out.resolve()
    corpora = args.corpus or ["language", "node-suite"]
    root = args.root.resolve()
    tmp = args.out / "tmp"
    tmp.mkdir(parents=True, exist_ok=True)

    env = dict(os.environ, PERRY_VERIFY_FACTS="1", PERRY_FACTS_UNITS="0", PERRY_NO_CACHE="1",
               PERRY_ALLOW_PERRY_FEATURES="1", TMPDIR=str(tmp))
    for k in ("PERRY_FACTS_SABOTAGE", "PERRY_FACTS_EXPLAIN", "PERRY_FACTS_MAX_WORK"):
        env.pop(k, None)

    programs = []
    for corpus in corpora:
        found = sorted((root / "tests" / "parity" / corpus).glob("*/*.ts"))
        if not found:
            sys.exit(f"no programs under tests/parity/{corpus}")
        programs += found

    def one(source):
        rel = source.relative_to(root)
        base = args.out / str(rel.with_suffix("")).replace("/", "_")
        exe = Path(str(base) + ".bin")
        clog = Path(str(base) + ".clog")
        err = Path(str(base) + ".err")
        try:
            with open(clog, "wb") as log:
                crc = subprocess.run([str(args.perry), "compile", str(rel), "-o", str(exe)], cwd=root, env=env,
                                     stdout=log, stderr=subprocess.STDOUT, timeout=args.compile_timeout).returncode
        except subprocess.TimeoutExpired:
            crc = 124
        rrc = None
        if exe.exists():
            try:
                with open(err, "wb") as log:
                    rrc = subprocess.run([str(exe)], cwd=source.parent, env=env, stdin=subprocess.DEVNULL,
                                         stdout=subprocess.DEVNULL, stderr=log, timeout=args.run_timeout).returncode
            except subprocess.TimeoutExpired:
                rrc = 124
        marks = sum(p.read_bytes().count(MARK) for p in (clog, err) if p.exists())
        false_proof = rrc == FALSE_PROOF_RC or marks > 0
        for p in (exe, Path(str(exe) + ".facts.jsonl")):
            p.unlink(missing_ok=True)
        if not false_proof and not args.keep:
            clog.unlink(missing_ok=True)
            err.unlink(missing_ok=True)
        return str(rel), crc, rrc, marks, false_proof

    with ThreadPoolExecutor(max_workers=args.jobs) as pool:
        rows = sorted(pool.map(one, programs))

    with open(args.out / "results.tsv", "w") as out:
        for rel, crc, rrc, marks, _ in rows:
            out.write(f"{rel}\t{crc}\t{'-' if rrc is None else rrc}\t{marks}\n")
    false_proofs = [r for r in rows if r[4]]
    compile_failures = [r for r in rows if r[1] != 0]
    for rel, crc, rrc, marks, _ in false_proofs:
        print(f"FALSE PROOF {rel} (run rc {rrc}, {marks} report lines; logs in {args.out})")
    for rel, crc, *_ in compile_failures:
        print(f"compile failure (rc {crc}) {rel}")
    print(f"{len(rows)} programs ({', '.join(corpora)}): {len(false_proofs)} false proofs, "
          f"{len(compile_failures)} compile failures")
    sys.exit(1 if false_proofs else 0)


if __name__ == "__main__":
    main()
