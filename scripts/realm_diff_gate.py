#!/usr/bin/env python3
"""Realm-diff gate (DESIGN §3.2, "The R-restricted realm-diff gate").

The realm dumper (`scripts/realm_dump.ts`) reflects over `globalThis`, which
makes its own realm reach R top: compiled plainly it always carries the full
image. Two modes compare its canonical descriptor graph with checked-in
references in `realm-dumps/`, byte for byte:

* R-restricted, per witness program (`realm-dumps/programs/<p>.ts`): the
  witness is compiled with `PERRY_REALM_REACH=1` and its R must equal the
  checked-in `<p>.names.json`. The dumper is then compiled with the test-only
  compiler-driver input `PERRY_REALM_REACH_NAMES=<p>.names.json`, which sets
  its R to exactly those names, top dropped. The gate asserts the
  input took effect (the driver printed `[realm-reach-names]` for this file,
  and the dumper's R has no top reason and is exactly those names), so a dumper
  built without it fails by construction. It walks only the roots in R (the
  names are handed to it as `realm-dump.names`), and its dump, restricted to
  the names, must equal `<p>.jsonl`, which is main's full walk restricted to
  the same names. Both dumpers are built from one source, the restricted one
  (`dumper_source(names)`): only R differs between the reference and the
  check. The plain source's namespace imports run every module's init, and
  those inits rewrite cells in R (console, process: own `prototype` keys and
  lengths; Math, SyntaxError.prototype), so a reference built from it
  recorded the dumper's own imports, not the realm.
* Full: the dumper compiled with no input (R is top); its dump must equal
  `full.jsonl`.

References are the base arm's dumps (main populates every cell). They change
only through the regeneration command, run with main's compiler:

    PERRY_RUNTIME_DIR=<main>/target/release \\
      python3 scripts/realm_diff_gate.py --perry <main>/target/release/perry --regenerate

`--self-test` feeds the plainly built dumper's compile log to the restricted
check, which must refuse it.
"""
import argparse
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile

sys.path.insert(0, str(Path(__file__).resolve().parent))
import realm_dump  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent
REFS = ROOT / "realm-dumps"
DUMPER = ROOT / "scripts" / "realm_dump.ts"
PROGRAMS = ["hello", "lad-04-classes", "lad-07-json", "node-imports"]
# The dump binary runs in one fixed environment on every arm and host.
RUN_ENV = {"PATH": "/usr/bin:/bin", "HOME": "/root", "LANG": "C"}
# The executable's path and the dumper's source path are realm values too
# (process.execPath, process.argv, module paths). Each gate run works in its
# own temporary directory, so concurrent runs never touch each other's files,
# and the dump spells that directory as a fixed token (paths derived from it,
# such as node:module's globalPaths entry <run>/../lib/node, follow).
WORK_TOKEN = "$REALM_DIFF_WORK"
_WORK = []


def work_dir():
    """This process's private directory: src/ holds the dumper source, run/
    the dump binary's working directory."""
    if not _WORK:
        work = Path(tempfile.mkdtemp(prefix="perry-realm-diff-"))
        import atexit
        atexit.register(shutil.rmtree, work, True)
        _WORK.append(work)
    return _WORK[0]


def import_name(specifier):
    """The name R records for `import * as ns from "<specifier>"`: the
    compiler's realm-reach scan (perry-hir realm_reach.rs native_module) adds
    the module's `node:<module>` namespace entry and its members. A native
    receiver cell (%NativeNamespace:...%) is a different root: the runtime
    spells it (path/posix's is %NativeNamespace:path.posix%) and some modules
    (node:fs) have none. The dumper keys its native roots by this name
    (`nativeRoots` in realm_dump.ts; the gate tests hold the two together)."""
    return "node:" + specifier.removeprefix("node:")


NATIVE_IMPORT = re.compile(r'^import \* as (realm_native_\d+) from "([^"]+)";$', re.M)


class GateError(Exception):
    pass


def compile_ts(perry, source, binary, extra_env):
    env = dict(os.environ)
    env.pop("PERRY_REALM_REACH_NAMES", None)
    env.update({"PERRY_REALM_REACH": "1", "PERRY_NO_CACHE": "1"})
    env.update(extra_env)
    binary.parent.mkdir(parents=True, exist_ok=True)
    run = subprocess.run([perry, "compile", str(source), "-o", str(binary)],
                         env=env, capture_output=True, text=True, cwd=binary.parent)
    log = run.stdout + run.stderr
    (binary.parent / (binary.name + ".compile.log")).write_text(log)
    if run.returncode != 0:
        raise GateError(f"{source.name} failed to compile (see {binary}.compile.log)")
    return log


def dumper_source(names=None):
    """The dumper, written to this run's src directory. Restricted to `names`, every
    namespace import of a module outside R is bound to undefined: a static
    import runs its module's init, which births all of the module's exports,
    and a restricted dumper must not touch a root outside R."""
    text = DUMPER.read_text()
    imports = NATIVE_IMPORT.findall(text)
    if not imports:
        raise GateError(f"{DUMPER} has no native namespace imports (format changed?)")
    if names is not None:
        text = NATIVE_IMPORT.sub(
            lambda m: m.group(0) if import_name(m.group(2)) in names
            else f"const {m.group(1)}: any = undefined; // {m.group(2)} is outside R", text)
    source = work_dir() / "src" / "realm_dump.ts"
    shutil.rmtree(source.parent, ignore_errors=True)
    source.parent.mkdir(parents=True)
    source.write_text(text)
    return source


def reach_of(log):
    lines = [l for l in log.splitlines() if l.startswith("[realm-reach-json] ")]
    if len(lines) != 1:
        raise GateError("expected one [realm-reach-json] line")
    return json.loads(lines[0].split(" ", 1)[1])


def check_restricted(log, names_file, names):
    """The dumper was built with this names file, and R is exactly partial."""
    took = [l.split() for l in log.splitlines() if l.startswith("[realm-reach-names] ")]
    if took != [["[realm-reach-names]", str(names_file), str(len(names))]]:
        raise GateError(f"the dumper was not built with PERRY_REALM_REACH_NAMES={names_file}")
    reach = reach_of(log)
    if reach["top"]:
        raise GateError(f"restricted dumper R is still top: {reach['top']}")
    exec_only = set(reach.get("exec_only", []))
    if set(reach["dumper_exec"]) <= set(reach["fixture_exec"]) and exec_only:
        raise GateError(f"fixture covers executing bodies but dumper adds {sorted(exec_only)}")
    expected = set(names) | exec_only
    if set(reach["names"]) != expected:
        extra = sorted(set(reach["names"]) - expected)[:10]
        missing = sorted(expected - set(reach["names"]))[:10]
        raise GateError(f"restricted dumper R is not the names file: extra {extra}, missing {missing}")


def dump(binary, names=None, walk=False):
    """Run a dumper; `walk` hands it the names (realm-dump.names) so a
    restricted dumper walks only roots in R. Its output is restricted to
    `names` either way."""
    # The executable's path and argv[0] are realm values (process.execPath,
    # process.argv, module paths): every dump runs as the same file name from
    # this run's directory, and that directory is spelled as a token.
    run_dir = work_dir() / "run"
    shutil.rmtree(run_dir, ignore_errors=True)
    run_dir.mkdir(parents=True)
    shutil.copy2(binary, run_dir / "realm-dump")
    if walk:
        (run_dir / "realm-dump.names").write_text("\n".join(sorted(names)))
    run = subprocess.run(["./realm-dump"], cwd=run_dir, env=RUN_ENV,
                         capture_output=True, text=True)
    if run.returncode != 0:
        raise GateError(f"{binary} exited {run.returncode}: {run.stderr[-500:]}")
    text = realm_dump.canonical(realm_dump.graph_from_text(run.stdout), names)
    return text.replace(str(work_dir()), WORK_TOKEN)


def witness_names(perry, program, out):
    log = compile_ts(perry, REFS / "programs" / f"{program}.ts", out / program / program, {})
    reach = reach_of(log)
    if reach["top"]:
        raise GateError(f"witness {program} selects top: {reach['top']}")
    names_file = out / program / "reach.names.json"
    names_file.write_text(json.dumps({"names":sorted(reach["names"]),"reach":reach["reach"]}, indent=2) + "\n")
    return sorted(reach["names"])


def compare(label, reference, actual, out):
    expected = reference.read_text() if reference.exists() else ""
    if actual == expected:
        print(f"realm_diff_gate: {label}: equal ({len(actual.splitlines())} cells)")
        return True
    (out / f"{label}.actual.jsonl").write_text(actual)
    import difflib
    diff = "".join(difflib.unified_diff(expected.splitlines(True), actual.splitlines(True),
                                        fromfile=str(reference), tofile=f"{label} (candidate)", n=1))
    print(f"FAIL {label}: differs from {reference}\n{diff[:4000]}")
    return False


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--perry", required=True)
    ap.add_argument("--output", type=Path, default=ROOT / "target" / "realm-diff")
    ap.add_argument("--regenerate", action="store_true", help="rewrite realm-dumps/ (base arm only)")
    ap.add_argument("--self-test", action="store_true")
    ap.add_argument("--only", nargs="*", help="limit to these programs and/or 'full'")
    args = ap.parse_args()
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=True)
    wanted = set(args.only) if args.only else set(PROGRAMS) | {"full"}
    ok = True
    try:
        for program in [p for p in PROGRAMS if p in wanted]:
            names = witness_names(args.perry, program, out)
            names_file = REFS / f"{program}.names.json"
            reference = REFS / f"{program}.jsonl"
            if args.regenerate:
                # Main populates every cell: its full realm (R top), dumped by
                # the restricted dumper's source and restricted to the names,
                # is the reference.
                names_file.write_text(json.dumps({"names": names}, indent=0) + "\n")
                binary = out / f"dump-{program}" / "realm-dump"
                compile_ts(args.perry, dumper_source(set(names)), binary, {})
                reference.write_text(dump(binary, set(names)))
                print(f"realm_diff_gate: regenerated {program} ({len(names)} names)")
                continue
            recorded = json.loads(names_file.read_text())["names"]
            if names != recorded:
                print(f"FAIL {program}: R is {len(names)} names, {names_file} records "
                      f"{len(recorded)}; added {sorted(set(names) - set(recorded))[:10]}, "
                      f"removed {sorted(set(recorded) - set(names))[:10]} (regenerate on main)")
                ok = False
                continue
            binary = out / f"dump-{program}" / "realm-dump"
            log = compile_ts(args.perry, dumper_source(set(names)), binary,
                             {"PERRY_REALM_REACH_NAMES": str(out / program / "reach.names.json")})
            check_restricted(log, out / program / "reach.names.json", names)
            ok &= compare(program, reference, dump(binary, set(names), walk=True), out)
        if "full" in wanted or args.self_test:
            binary = out / "dump-full" / "realm-dump"
            log = compile_ts(args.perry, dumper_source(), binary, {})
            if not reach_of(log)["top"]:
                raise GateError("the plainly built dumper's R is not top")
            if args.regenerate:
                (REFS / "full.jsonl").write_text(dump(binary))
                print("realm_diff_gate: regenerated full")
            elif "full" in wanted:
                ok &= compare("full", REFS / "full.jsonl", dump(binary), out)
            if args.self_test:
                names_file = REFS / "hello.names.json"
                names = json.loads(names_file.read_text())["names"]
                try:
                    check_restricted(log, names_file, names)
                    print("FAIL self-test: a dumper built without the input passed the restricted check")
                    ok = False
                except GateError as e:
                    print(f"realm_diff_gate: self-test ok: the restricted check refused the plain dumper ({e})")
    except GateError as e:
        print(f"FAIL {e}")
        ok = False
    if not ok:
        sys.exit(1)
    print("realm_diff_gate: ok")


if __name__ == "__main__":
    main()
