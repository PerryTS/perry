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
  its R to those names united with its own, top dropped. The gate asserts the
  input took effect (the driver printed `[realm-reach-names]` for this file,
  and the dumper's R has no top reason and is the closure of exactly those
  names united with the dumper's own pre-closure names), so a dumper
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

The full dumper also writes one identity line per native namespace
declaration (does every access path yield one object). The cells that still
mint a fresh object per access on main are listed in MINTING; the gate
asserts the dump's minting set equals that list, so a cell that stops minting
or starts is red. The static realm image (WRT PR B) empties the list.

The agreement check (A2-DESIGN 6.4): a native namespace has one member list,
its declaration, and `Object.keys` reads it, so the declared order and the
observed order are one list and agreeing with Node is one comparison per cell.
The full dumper writes one `@order` line per static import's module value and
per sub-namespace; the gate compares the keys both sides have, in order, with
Node's `Object.keys` over the same access path, recorded in
`native-order.node.json` (Node v26.12.1). A cell Node has no value for is
recorded as null there and listed by name, never skipped; a cell with no entry
is red. Disagreeing cells must equal ORDER_DISAGREES: a cell that starts or stops
disagreeing is red.
The Node reference changes only through

    python3 scripts/realm_diff_gate.py --perry <perry> --only full --regenerate-node --node <node v26.12.1>

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
    spells it (path/posix's is %NativeNamespace:path.posix%, not its
    specifier). The dumper keys its native roots by this name
    (`nativeRoots` in realm_dump.ts; the gate tests hold the two together)."""
    return "node:" + specifier.removeprefix("node:")


NATIVE_IMPORT = re.compile(r'^import \* as (realm_native_\d+) from "([^"]+)";$', re.M)


# A2-DESIGN §9 Q6: 11 namespaces and 6 sub-namespaces mint per access.
MINTING = sorted(["console"] + ["%NativeNamespace:" + m + "%" for m in [
    "buffer", "crypto", "domain", "readline", "string_decoder", "tls", "tty", "v8",
    "worker_threads", "zlib"]] + ["node:buffer.constants", "node:crypto.constants",
    "node:http2.constants", "node:perf_hooks.constants", "node:v8.promiseHooks",
    "node:v8.startupSnapshot"])


NODE_ORDER = REFS / "native-order.node.json"
# Cells whose shared members are in a different relative order than Node's.
# None is one of the A2 namespace declarations: the module value is a function
# with own statics (assert, events, module), a submodule export object
# (diagnostics_channel, fs/promises, stream/consumers, stream/web,
# test/reporters) or the util object served for sys. Each leaves the list when
# its export order is fixed forward.
ORDER_DISAGREES = sorted([
    "node:assert", "node:diagnostics_channel", "node:events", "node:fs/promises", "node:module",
    "node:stream/consumers", "node:stream/web", "node:sys", "node:test/reporters"])


class GateError(Exception):
    pass


def identities(stdout):
    return [json.loads(line[len("@identity "):]) for line in stdout.splitlines()
            if line.startswith("@identity ")]


def check_identity(records):
    """Each declaration is recorded once; the minting set equals MINTING."""
    names = [r["name"] for r in records]
    if not names or len(set(names)) != len(names):
        raise GateError(f"identity records are missing or repeat a declaration: {names}")
    minting = sorted(r["name"] for r in records if not r["one"])
    if minting != MINTING:
        raise GateError(f"minting set differs from MINTING: starts {sorted(set(minting) - set(MINTING))}, "
                        f"stops {sorted(set(MINTING) - set(minting))}")
    return minting


def orders(stdout):
    return [json.loads(line[len("@order "):]) for line in stdout.splitlines()
            if line.startswith("@order ")]


def shared_order(keys, other):
    both = set(keys) & set(other)
    return [k for k in keys if k in both]


def check_order(records, node):
    """Every cell has a Node entry; a null entry is a cell with no Node value
    (listed); every other cell's shared members are in Node's relative order,
    except exactly ORDER_DISAGREES."""
    names = [r["name"] for r in records]
    if not names or len(set(names)) != len(names):
        raise GateError(f"order records are missing or repeat a cell: {names}")
    missing = [n for n in names if n not in node]
    if missing:
        raise GateError(f"cells with no entry in {NODE_ORDER.name} (regenerate with node): {missing}")
    no_node, agree, disagree = [], [], []
    for r in records:
        theirs = node[r["name"]]
        if theirs is None or r["keys"] is None:
            no_node.append(r["name"])
        elif shared_order(r["keys"], theirs) == shared_order(theirs, r["keys"]):
            agree.append(r["name"])
        else:
            disagree.append(r["name"])
    if sorted(disagree) != sorted(ORDER_DISAGREES):
        raise GateError(f"member order differs from Node's beyond ORDER_DISAGREES: starts "
                        f"{sorted(set(disagree) - set(ORDER_DISAGREES))}, stops "
                        f"{sorted(set(ORDER_DISAGREES) - set(disagree))}")
    return agree, no_node


NODE_ORDER_PROBE = r"""
const subjects = JSON.parse(require("fs").readFileSync(0, "utf8"));
const out = {};
for (const s of subjects) {
  let value;
  try { const root = require(s.spec); value = s.key === "" ? root : root[s.key]; } catch { value = undefined; }
  out[s.name] = (typeof value === "object" && value !== null) || typeof value === "function" ? Object.keys(value) : null;
}
process.stdout.write(JSON.stringify(out, null, 0));
"""


def node_order(node, records):
    """Node's Object.keys over each record's access path (null: no value)."""
    subjects = [{"name": r["name"], "spec": r["spec"], "key": r["key"]} for r in records]
    run = subprocess.run([node, "-e", NODE_ORDER_PROBE], input=json.dumps(subjects),
                         capture_output=True, text=True, env=RUN_ENV)
    if run.returncode != 0:
        raise GateError(f"node order probe failed: {run.stderr[-500:]}")
    return json.loads(run.stdout)


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
    """The dumper was built with this names file, and R is the closure of
    exactly the file's names united with the dumper's own (pre-closure) names:
    the seed the compiler closed is that union, nothing else, and R holds it."""
    took = [l.split() for l in log.splitlines() if l.startswith("[realm-reach-names] ")]
    if took != [["[realm-reach-names]", str(names_file), str(len(names))]]:
        raise GateError(f"the dumper was not built with PERRY_REALM_REACH_NAMES={names_file}")
    reach = reach_of(log)
    if reach["top"]:
        raise GateError(f"restricted dumper R is still top: {reach['top']}")
    union = set(names) | set(reach["own"])
    seed = set(reach["seed"] or ())
    if seed != union:
        raise GateError(f"restricted dumper R is not the names file united with its own names: "
                        f"extra {sorted(seed - union)[:10]}, missing {sorted(union - seed)[:10]}")
    if not union <= set(reach["names"]):
        raise GateError(f"restricted dumper R lacks {sorted(union - set(reach['names']))[:10]}")
    return reach


def dump(binary, names=None, walk=False, records=None, order=None):
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
    if records is not None:
        records.extend(identities(run.stdout))
    if order is not None:
        order.extend(orders(run.stdout))
    return text.replace(str(work_dir()), WORK_TOKEN)


def witness_names(perry, program, out):
    log = compile_ts(perry, REFS / "programs" / f"{program}.ts", out / program / program, {})
    reach = reach_of(log)
    if reach["top"]:
        raise GateError(f"witness {program} selects top: {reach['top']}")
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
    ap.add_argument("--regenerate-node", action="store_true",
                    help=f"rewrite realm-dumps/{NODE_ORDER.name} from --node (Node v26.12.1)")
    ap.add_argument("--node", help="the node binary for --regenerate-node")
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
                             {"PERRY_REALM_REACH_NAMES": str(names_file)})
            reach = check_restricted(log, names_file, names)
            print(f"realm_diff_gate: {program}: R = {len(reach['names'])} names "
                  f"(file {len(names)}, the dumper's own {len(reach['own'])})")
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
                records, order = [], []
                text = dump(binary, records=records, order=order)
                if args.regenerate_node:
                    if not args.node:
                        raise GateError("--regenerate-node needs --node")
                    NODE_ORDER.write_text(json.dumps(node_order(args.node, order), indent=0, sort_keys=True) + "\n")
                    print(f"realm_diff_gate: regenerated {NODE_ORDER.name} ({len(order)} cells)")
                ok &= compare("full", REFS / "full.jsonl", text, out)
                minting = check_identity(records)
                print(f"realm_diff_gate: identity: {len(records)} declarations, "
                      f"{len(minting)} minting = MINTING")
                agree, no_node = check_order(order, json.loads(NODE_ORDER.read_text()))
                print(f"realm_diff_gate: order: {len(agree)} cells agree with Node, "
                      f"{len(ORDER_DISAGREES)} in ORDER_DISAGREES, no Node value: {no_node}")
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
