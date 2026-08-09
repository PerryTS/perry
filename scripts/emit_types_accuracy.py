#!/usr/bin/env python3
"""Measure `--emit-types` (#7685, EXPERIMENTAL) instead of demonstrating it.

Two modes, because the two corpora can answer different questions:

``roundtrip``
    Take annotated ``.ts`` inputs, **erase** the local annotations, compile the
    erased copy, and diff what came back against what was erased. That is an
    accuracy number.

    The erasure is not a formality — it is the entire validity of the measure.
    `stmt/let_stmt.rs` computes a local's `refined_ty` as *the declared type
    when it is not `Any`*, falling back to inference from the initializer. Run
    this against un-erased sources and every "recovered" type is the annotation
    being handed straight back, and the score is 100% and means nothing.

``coverage``
    Real dependency JavaScript has no annotations, so there is no ground truth
    and no accuracy to compute. What can be measured is how many bindings get a
    type at all. Reported as a rate against a syntactic count of local
    declarations, which is approximate — and labelled as approximate.

Both modes exit non-zero if they measured **zero files**, and treat a file whose
compile produced no report as a hard error rather than a zero. An accuracy
number over an empty set is worse than no number.
"""

from __future__ import annotations

import argparse
import json
import pathlib
import re
import shutil
import subprocess
import sys
import tempfile

# `let x: T = ...` / `const x: T = ...`. Deliberately narrow: it matches the
# single-line, non-generic-comma annotations that make up the ground truth, and
# skips anything it cannot read rather than guessing. A missed annotation costs
# denominator, never correctness — an annotation this does not extract simply is
# not scored.
DECL_ANNOTATED = re.compile(
    r"\b(?:let|const|var)\s+([A-Za-z_$][\w$]*)\s*:\s*"
    r"([A-Za-z_$][\w$]*(?:\s*\[\s*\])?|[A-Za-z_$][\w$]*<[^<>,()]*>)\s*="
)
# Any local declaration, annotated or not — the coverage denominator.
DECL_ANY = re.compile(r"\b(?:let|const|var)\s+([A-Za-z_$][\w$]*)\s*[:=]")


class CompileFailed(Exception):
    """The input does not compile standalone — skipped, not a measurement.

    Kept distinct from every other failure so that "Perry cannot build this
    file" can never be silently folded into "Perry proved nothing here".
    """


def normalize_type(text: str) -> str:
    """Canonical spelling so `Array<number>` and `number[]` compare equal."""
    t = " ".join(text.split())
    m = re.fullmatch(r"Array<\s*(.+?)\s*>", t)
    if m:
        t = f"{m.group(1)}[]"
    return t.replace(" [ ]", "[]").replace("[ ]", "[]")


def erase_local_annotations(source: str) -> tuple[str, dict[str, str], set[str]]:
    """Strip `: T` from local declarations.

    Returns the erased source, the ground truth it removed (keyed by binding
    name), and the set of names whose annotation was `any`/`unknown`.

    A name declared twice with different types is dropped from the truth rather
    than resolved: this harness scores by name (the report carries no span), so
    an ambiguous name cannot be scored honestly either way.
    """
    truth: dict[str, str] = {}
    ambiguous: set[str] = set()
    improved_on_any: set[str] = set()

    def repl(m: re.Match[str]) -> str:
        name, ty = m.group(1), normalize_type(m.group(2))
        if name in truth and truth[name] != ty:
            ambiguous.add(name)
        # `any` / `unknown` are the ABSENCE of a claim, not a claim. Scoring
        # against them would count "we recovered a structural interface for a
        # local the author gave up on" as a WRONG answer — which is backwards,
        # and is exactly what the first run of this harness did report.
        # They are erased like any other annotation (so the compiler cannot
        # read them) but excluded from the ground truth, and counted
        # separately as an improvement.
        if ty not in ("any", "unknown"):
            truth[name] = ty
        else:
            ambiguous.discard(name)
            improved_on_any.add(name)
        # Rebuild the declaration without its annotation: keep everything up to
        # the name, then go straight to the `=`.
        head = m.group(0)[: m.start(1) - m.start(0)]
        return f"{head}{name} ="

    erased = DECL_ANNOTATED.sub(repl, source)
    for name in ambiguous:
        truth.pop(name, None)
    return erased, truth, improved_on_any


def compile_and_emit(perry: str, src: pathlib.Path, workdir: pathlib.Path) -> dict:
    """Compile one file and return the parsed `--emit-types` JSON.

    Raises on anything that would let a broken run masquerade as a zero.
    """
    out_json = workdir / "types.json"
    obj = workdir / "out.o"
    cmd = [
        perry,
        "compile",
        str(src),
        "--emit-types",
        str(out_json),
        "--no-link",
        "-o",
        str(obj),
    ]
    env = {"PERRY_NO_AUTO_OPTIMIZE": "1", "PATH": "/usr/bin:/bin:/usr/sbin:/sbin"}
    proc = subprocess.run(
        cmd, capture_output=True, text=True, env=env, timeout=300, cwd=str(workdir)
    )
    if proc.returncode != 0:
        # A file that does not compile standalone (an unresolved import, a
        # feature Perry lacks) is not a measurement — it is skipped and
        # counted. Distinct from the fatal case below.
        raise CompileFailed(
            f"compile failed ({proc.returncode}): "
            f"{proc.stderr.strip().splitlines()[-1] if proc.stderr.strip() else 'no stderr'}"
        )
    if not out_json.exists():
        # Compiled fine and produced NOTHING. That is the vacuity case: it must
        # never be recorded as "zero types recovered", because a silently
        # inert flag and a genuinely untypeable program are the same number.
        raise RuntimeError(
            f"{src.name}: compile succeeded but wrote no types file — the flag "
            f"did not run. This is a harness/compiler error, NOT a zero."
        )
    return json.loads(out_json.read_text())


def run_roundtrip(perry: str, inputs: list[pathlib.Path]) -> int:
    exact = mismatch = 0
    recovered_total = 0
    truth_total = 0
    files_measured = 0
    files_zero: list[str] = []
    files_failed: list[str] = []
    mismatches: list[str] = []

    improved = 0
    for src in inputs:
        source = src.read_text()
        erased, truth, any_names = erase_local_annotations(source)
        if not truth and not any_names:
            continue  # nothing annotated to score; not a measured file
        with tempfile.TemporaryDirectory() as td:
            work = pathlib.Path(td)
            target = work / src.name
            target.write_text(erased)
            try:
                doc = compile_and_emit(perry, target, work)
            except CompileFailed as exc:
                files_failed.append(f"{src.name}: {exc}")
                continue
            except Exception as exc:  # noqa: BLE001 - fatal by design
                print(f"  FATAL {src.name}: {exc}", file=sys.stderr)
                return 2

        files_measured += 1
        truth_total += len(truth)
        bindings = doc.get("bindings", [])
        recovered_total += len(bindings)
        if not bindings:
            files_zero.append(src.name)
        for b in bindings:
            if b["name"] in any_names:
                improved += 1
                continue
            want = truth.get(b["name"])
            if want is None:
                continue  # recovered a binding that carried no annotation
            if normalize_type(b["ts_type"]) == want:
                exact += 1
            else:
                mismatch += 1
                mismatches.append(
                    f"{src.name}:{b['function']}:{b['name']} "
                    f"want={want} got={b['ts_type']} ({b['analysis']}/{b['rep']})"
                )

    if files_measured == 0:
        print("FAIL: measured zero files — nothing was scored.", file=sys.stderr)
        return 2

    scored = exact + mismatch
    print(f"files measured:        {files_measured}")
    print(f"files skipped (build): {len(files_failed)}")
    print(f"  of which 0 recovered:{len(files_zero)}")
    print(f"erased annotations:    {truth_total}   (the ground truth)")
    print(f"bindings recovered:    {recovered_total}")
    print(f"  scored against truth:{scored}")
    print(f"    exact match:       {exact}")
    print(f"    MISMATCH:          {mismatch}")
    print(f"  improved on `any`:   {improved}  (source said `any`; we proved a type)")
    if truth_total:
        print(f"recall  (exact/truth): {exact / truth_total:6.1%}")
    if scored:
        print(f"precision (exact/scored): {exact / scored:6.1%}")
    for m in mismatches:
        print(f"    ! {m}")
    # A mismatch is a potential wrong-type bug — the one thing this feature
    # promises never to do. It fails the run so it cannot be scrolled past.
    return 1 if mismatch else 0


def run_coverage(perry: str, inputs: list[pathlib.Path]) -> int:
    files_measured = 0
    files_failed: list[str] = []
    decls_total = 0
    recovered_total = 0
    shapes_total = 0
    per_file: list[tuple[str, int, int]] = []

    for src in inputs:
        try:
            source = src.read_text(errors="replace")
        except OSError:
            continue
        decls = len(set(DECL_ANY.findall(source)))
        with tempfile.TemporaryDirectory() as td:
            work = pathlib.Path(td)
            target = work / src.name
            shutil.copyfile(src, target)
            try:
                doc = compile_and_emit(perry, target, work)
            except CompileFailed as exc:
                files_failed.append(f"{src.name}: {str(exc)[:160]}")
                continue
            except Exception as exc:  # noqa: BLE001 - fatal by design
                # "Compiled but emitted nothing" is the vacuity case and must
                # stop the run, not be counted as a build failure.
                print(f"  FATAL {src.name}: {exc}", file=sys.stderr)
                return 2
        files_measured += 1
        decls_total += decls
        n = len(doc.get("bindings", []))
        recovered_total += n
        shapes_total += len(doc.get("shapes", []))
        per_file.append((src.name, decls, n))

    if files_measured == 0:
        print(
            f"FAIL: measured zero files ({len(files_failed)} failed to compile).",
            file=sys.stderr,
        )
        for f in files_failed[:20]:
            print(f"  {f}", file=sys.stderr)
        return 2

    print(f"files compiled OK:     {files_measured}")
    print(f"files failed to build: {len(files_failed)}")
    print(f"local declarations:    {decls_total}   (syntactic count — APPROXIMATE)")
    print(f"bindings recovered:    {recovered_total}")
    print(f"structural shapes:     {shapes_total}")
    if decls_total:
        print(f"coverage:              {recovered_total / decls_total:6.2%}")
    print("\nper file (name, decls, recovered):")
    for name, d, n in sorted(per_file, key=lambda r: -r[2])[:25]:
        print(f"  {n:5d} / {d:5d}  {name}")
    for f in files_failed[:15]:
        print(f"  BUILD-FAIL {f}")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--perry", required=True, help="path to the perry binary")
    ap.add_argument("--mode", choices=("roundtrip", "coverage"), required=True)
    ap.add_argument("--inputs", nargs="+", required=True, help="files or directories")
    ap.add_argument("--limit", type=int, default=0, help="cap files (0 = no cap)")
    args = ap.parse_args()

    files: list[pathlib.Path] = []
    exts = (".ts",) if args.mode == "roundtrip" else (".js", ".cjs", ".mjs")
    for raw in args.inputs:
        p = pathlib.Path(raw)
        if p.is_dir():
            files.extend(
                sorted(f for f in p.rglob("*") if f.suffix in exts and f.is_file())
            )
        elif p.is_file():
            files.append(p)
    if args.limit:
        files = files[: args.limit]
    if not files:
        print("FAIL: no input files matched.", file=sys.stderr)
        return 2

    if args.mode == "roundtrip":
        return run_roundtrip(args.perry, files)
    return run_coverage(args.perry, files)


if __name__ == "__main__":
    sys.exit(main())
