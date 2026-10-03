#!/usr/bin/env python3
"""Run after the owner releases the heavy slot; never build Cargo archives.

Requires a truthful build receipt produced for the frozen tested source. Receipt
schema: commit, compiler_sha256, runtime_sha256, stdlib_sha256. The runner verifies
artifact hashes, uses only the supplied full gc-instruments runtime, and prevents
implicit Cargo builds. Logs/artifacts stay under an exclusive output directory.
"""
import argparse
import collections
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys

EXPECTED = {
    "factory": "94208 true 17 29\n103 29\n202 29\nundefined 29\n",
    "classes": "280064 523 535 true\n101 535\n8128 8128\n",
    "numeric-loops": "4097 2 1639600 256 63.5\n65024\n42\n24384\n",
    "numeric-recheck": "1200 200 512 200\n",
    "cache": "17 94208 105 true\n",
    "workers-before": "17 29 17 29\n5 6\n",
    "workers-after": "17 29\n17 29 17 29\n5 6\n",
}
STATIC = re.compile(r"perry-static-shape: (\S+) requested=(0x[0-9a-f]+) got=(0x[0-9a-f]+) (hit|miss)")
CACHE = re.compile(r"codegen cache: (\d+)/(\d+) hit \((\d+) miss\)")


def require(condition, reason):
    if not condition:
        raise RuntimeError(reason)


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def run(command, env, cwd, log):
    p = subprocess.run(command, env=env, cwd=cwd, capture_output=True, text=True)
    log.with_suffix(".stdout").write_text(p.stdout)
    log.with_suffix(".stderr").write_text(p.stderr)
    return p


def good(p, label):
    require(p.returncode == 0, f"{label}: rc={p.returncode}; see captured logs")


def census(stderr, label, *, constfn, worker=False):
    records = STATIC.findall(stderr)
    require(records, f"{label}: static census never ran")
    rebuilt = []
    for path, requested, got, verdict in records:
        if int(requested, 16) == 0:
            # Receiver reconstruction asks the interner for a shape by facts,
            # not for a particular static id. Its diagnostic therefore says
            # requested=0, got=<real id>, miss; that is not an id contradiction.
            require(path == "finalized-constfn" and int(got, 16) > 0 and verdict == "miss",
                    f"{label}: malformed dynamic finalization census")
            rebuilt.append((path, requested, got, verdict))
        else:
            require(requested == got and verdict == "hit", f"{label}: static-id mismatch")
    promoted = any(path == "finalized-constfn" for path, *_ in records)
    require(promoted == constfn, f"{label}: completed receiver witness differs from arm")
    if worker and constfn:
        require(rebuilt, f"{label}: receiver reconstruction did not publish ConstFn facts")
    return records


def witnesses(stderr, constfn, label, calls=None):
    records = census(stderr, label, constfn=constfn)
    matches = re.findall(r"\[method-site\][^\n]*", stderr)
    primes = [int(x) for line in matches for x in re.findall(r"primes_constfn=(\d+)", line)]
    if constfn:
        require(primes and max(primes) > 0, f"{label}: no ConstFn own entry primed")
        if calls:
            misses = [int(x) for line in matches for x in re.findall(r"\bmisses=(\d+)", line)]
            require(misses and max(misses) < calls, f"{label}: no repeated inline hit witness")
    else:
        require(not any(primes), f"{label}: off arm admitted ConstFn")
    return collections.Counter(records)



def numeric_routes(stderr, label, *, constfn, refusal=False):
    stores = [dict(re.findall(r"([\w.]+)=(\d+)", line))
              for line in re.findall(r"\[store-census\][^\n]*", stderr)]
    regions = [dict(re.findall(r"([\w.]+)=(\d+)", line))
               for line in re.findall(r"^PERRY_RECV_ROUTES[^\n]*", stderr, re.MULTILINE)]
    require(stores and regions, f"{label}: numeric route instruments did not report")
    require(len(stores) == len(regions) == 1, f"{label}: ambiguous numeric route census")
    counters = {key: int(value) for row in (stores[0], regions[0]) for key, value in row.items()}
    require(counters.get("emit.cfield.loop_raw_store", 0) == 0,
            f"{label}: legacy class numeric store executed")
    positive = ("rloop_static", "rloop_f", "rloop_f_rep", "rloop_bare",
                "emit.elem.read.versioned_indexed")
    if refusal:
        # The original control mutates Pair/entities, never LoopCell. Its
        # unaffected store-only loop must keep exactly its own 200 admissions.
        # The isolated controls below reject affected raw accesses separately.
        for key, expected in (("rloop_f", 200), ("rloop_f_rep", 200),
                              ("rloop_bare", 200), ("emit.elem.read.versioned_indexed", 0)):
            require(counters.get(key, 0) == expected,
                    f"{label}: only untouched LoopCell may admit {key}={expected}: {counters}")
        require(counters.get("rloop_static", 0) > 0 and counters.get("rloop_g", 0) > 0,
                f"{label}: unaffected cell/affected Pair witnesses are missing")
        require(sum(counters.get(key, 0) for key in
                    ("emit.cfield.ic_call", "emit.cfield.guard_fallback_call",
                     "emit.elem.read.fallback_call", "emit.elem.read.cold_arm")) > 0,
                f"{label}: refusal did not execute a generic fallback")
    else:
        for key in positive:
            require(counters.get(key, 0) > 0, f"{label}: numeric positive route never executed: {key}")
    if constfn:
        records = census(stderr, label, constfn=True)
        classes = {got for path, _, got, _ in records if path == "class-constfn"}
        finalized = {got for path, _, got, _ in records if path == "finalized-constfn"}
        require(len(classes) == 2 and classes <= finalized,
                f"{label}: both Pair and Reader must actually finalize, not merely seed")
    return counters


def recheck_routes(stderr, label, *, mutated=False):
    rows = [dict(re.findall(r"([\w.]+)=(\d+)", line))
            for line in re.findall(r"^PERRY_RECV_ROUTES[^\n]*", stderr, re.MULTILINE)]
    require(len(rows) == 1, f"{label}: ambiguous or missing recheck route census")
    counters = {key: int(value) for key, value in rows[0].items()}
    fast, generic = (100, 100) if mutated else (200, 0)
    require(counters.get("rloop_f", 0) == fast and counters.get("rloop_g", 0) == generic,
            f"{label}: expected F={fast}, G={generic}; got {counters}")
    require(counters.get("rloop_bare", 0) == 2 * fast
            and counters.get("rloop_f_rep", 0) == fast
            and counters.get("rloop_recheck", 0) > 0,
            f"{label}: exact numeric reads/recheck did not execute")
    return counters


def arithmetic_region_formation(ir):
    """Require the arithmetic fixture's F body, separately from other tiers."""
    functions = [(name.strip('"'), body) for name, body in re.findall(
        r'^define[^\n]*@([^\s(]+)[^\n]*\{\n(.*?)^\}', ir, re.MULTILINE | re.DOTALL)]
    bodies = [body for name, body in functions
              if name.startswith("perry_fn_") and name.endswith("__regionRead")]
    require(len(bodies) == 1, "numeric trace must contain exactly one regionRead body")
    body = bodies[0]
    blocks = {}
    current = None
    for line in body.splitlines():
        label = re.match(r'^([\w.]+):(?:\s*;.*)?$', line)
        if label:
            current = label[1]
            blocks[current] = []
        elif current is not None and line.strip():
            blocks[current].append(line.strip())
    work = [label for label in blocks if label.startswith("rloop.fast")]
    require(work, "arithmetic regionRead never formed F")
    seen = set()
    while work:
        label = work.pop()
        if label.startswith("rloop.join") or label in seen:
            continue
        require(label in blocks, f"unresolved arithmetic F successor: {label}")
        seen.add(label)
        instructions = blocks[label]
        if instructions:
            work.extend(re.findall(r'label %([\w.]+)', instructions[-1]))
    fast = "\n".join(line for label in seen for line in blocks[label])
    formation = {
        "arithmetic F formed": bool(seen),
        "arithmetic F multiplies": "fmul double" in fast,
        "arithmetic F adds": "fadd double" in fast,
        "arithmetic R witness emitted": bool(re.search(r'@js_recv_route_note\(i32 34\)', fast)),
        "arithmetic F uses exact bare reads": "load double" in fast and "@js_object_get_field" not in fast,
        "arithmetic F has no dynamic add": "@js_dynamic_string_or_number_add" not in fast,
        "arithmetic F has no fact tree": "rloop.tree" not in fast,
        "arithmetic F needs no recheck": "rloop.recheck" not in body,
    }
    require(all(formation.values()), f"arithmetic region formation failed: {formation}")
    return formation

def class_store_routes(stderr, label):
    rows = [dict(re.findall(r"([\w.]+)=(\d+)", line))
            for line in re.findall(r"^PERRY_RECV_ROUTES[^\n]*", stderr, re.MULTILINE)]
    require(len(rows) == 1, f"{label}: missing or ambiguous replacement routes")
    counters = {key: int(value) for key, value in rows[0].items()}
    for key, expected in (("rloop_f", 200), ("rloop_f_rep", 200),
                          ("rloop_bare", 400), ("rloop_g", 0), ("rloop_recheck", 0)):
        require(counters.get(key, 0) == expected,
                f"{label}: isolated class increment needs {key}={expected}: {counters}")
    stores = re.findall(r"\[store-census\][^\n]*", stderr)
    require(len(stores) == 1, f"{label}: missing or ambiguous store census")
    old = dict(re.findall(r"([\w.]+)=(\d+)", stores[0]))
    require(int(old.get("emit.cfield.loop_raw_store", 0)) == 0,
            f"{label}: legacy class increment executed")
    return counters


def guarded_fast_body(body, label):
    """F may enter unconditionally inside a conditionally versioned loop."""
    blocks = {}
    current = None
    for line in body.splitlines():
        match = re.match(r'^([\w.]+):(?:\s*;.*)?$', line)
        if match:
            current = match[1]
            blocks[current] = []
        elif current is not None and line.strip() and not line.lstrip().startswith(";"):
            blocks[current].append(line.strip())
    require(blocks, f"{label}: no function CFG")

    def successors(block):
        if not blocks[block]:
            return []
        terminal = blocks[block][-1]
        constant = re.match(r'br i1 (true|false), label %([\w.]+), label %([\w.]+)', terminal)
        if constant:
            return [constant[2] if constant[1] == "true" else constant[3]]
        return re.findall(r'label %([\w.]+)', terminal)

    def reachable(starts, stop_at_join=False):
        work, seen = list(starts), set()
        while work:
            block = work.pop()
            if block in seen or (stop_at_join and block.startswith(("rloop.join", "rloop.version.merge"))):
                continue
            require(block in blocks, f"{label}: unresolved CFG successor {block}")
            seen.add(block)
            work.extend(successors(block))
        return seen

    live = reachable([next(iter(blocks))])
    starts = {block for block in live if block.startswith("rloop.fast")}
    require(starts, f"{label}: F is absent or unreachable from function entry")
    entry = next(iter(blocks))
    predecessors = {block: set() for block in live}
    for block in live:
        for successor in successors(block):
            predecessors[successor].add(block)
    dominators = {block: ({entry} if block == entry else set(live)) for block in live}
    changed = True
    while changed:
        changed = False
        for block in live - {entry}:
            incoming = predecessors[block]
            common = set.intersection(*(dominators[pred] for pred in incoming)) if incoming else set()
            updated = {block} | common
            if updated != dominators[block]:
                dominators[block] = updated
                changed = True
    guarded = set()
    for block in live:
        if not block.startswith(("rloop.guard", "rloop.version")) or not blocks[block]:
            continue
        branch = re.match(r'br i1 (?!true,|false,)[^,]+, label %([\w.]+), label %([\w.]+)', blocks[block][-1])
        if branch:
            first = starts & reachable([branch[1]], True)
            second = starts & reachable([branch[2]], True)
            guarded.update(fast for fast in first ^ second if block in dominators[fast])
    require(starts <= guarded, f"{label}: conditional region/version guard does not dominate F")
    fast = reachable(starts, True)
    return "\n".join(line for block in fast for line in blocks[block])


def class_store_region_formation(ir):
    functions = [(name.strip('"'), body) for name, body in re.findall(
        r'^define[^\n]*@([^\s(]+)[^\n]*\{\n(.*?)^\}', ir, re.MULTILINE | re.DOTALL)]
    bodies = [body for name, body in functions
              if name.startswith("perry_fn_") and name.endswith("__classLoopReplacement")]
    require(len(bodies) == 1, "replacement trace needs one classLoopReplacement function")
    body = bodies[0]
    fast = guarded_fast_body(body, "class replacement")
    require("rloop.guard." in body and "rloop.recheck" not in body,
            "replacement needs a generic region guard without a recheck cliff")
    require("@js_recv_route_note(i32 34)" in fast,
            "replacement store function has no R witness")
    require(fast.count("@js_recv_route_note(i32 18)") == 2,
            "replacement needs exactly its bare field read and bare store")
    require("load double" in fast and "store double" in fast and "fadd double" in fast,
            "replacement F must read, add, and commit a raw store")
    require(not any(marker in fast for marker in
                    ("rloop.tree", "@js_dynamic_string_or_number_add", "@js_object_get_field",
                     "@js_typed_feedback_class_field", "@js_gc_write_barrier", "@js_put_value")),
            "replacement F retained ordinary numeric or store bookkeeping")
    require(not any(marker in ir for marker in
                    ("class_field.loop.", "class_field_loop.", "class_field_loop_store.",
                     "for.class_field_fast", "for.class_field_slow")),
            "replacement IR retained the legacy numeric tier")
    return {"isolated class R/store formed": True, "isolated legacy absent": True}


def isolated_refusal_routes(stderr, label, *, kind, mutated=True):
    """One executable isolates one affected receiver; no unrelated F can hide it."""
    stores = re.findall(r"\[store-census\][^\n]*", stderr)
    regions = re.findall(r"^PERRY_RECV_ROUTES[^\n]*", stderr, re.MULTILINE)
    require(len(stores) == len(regions) == 1, f"{label}: missing/ambiguous isolated census")
    counters = {key: int(value) for line in stores + regions
                for key, value in re.findall(r"([\w.]+)=(\d+)", line)}
    require(counters.get("emit.cfield.loop_raw_store", 0) == 0,
            f"{label}: legacy store executed")
    fast = 200 if kind == "cell" and not mutated else 0
    for key in ("rloop_f", "rloop_f_rep", "rloop_bare"):
        require(counters.get(key, 0) == fast,
                f"{label}: isolated {kind} needs {key}={fast}: {counters}")
    require(counters.get("emit.elem.read.versioned_indexed", 0) == 0,
            f"{label}: affected indexed receiver entered the versioned route")
    if fast:
        require(counters.get("rloop_g", 0) == 0 and counters.get("rloop_recheck", 0) == 0
                and counters.get("rloop_static", 0) > 0,
                f"{label}: untouched cell did not use its static store-only region")
    else:
        fallback = ("emit.elem.read.fallback_call", "emit.elem.read.cold_arm") if kind == "reader" else (
            "emit.cfield.ic_call", "emit.cfield.guard_fallback_call", "emit.by_name.put_value",
            "emit.by_name.runtime")
        require(sum(counters.get(key, 0) for key in fallback) > 0,
                f"{label}: no affected ordinary fallback execution")
        if kind != "reader":
            expected = 400 if kind == "all" else 200
            # These static-supplier loops reject before versioning. They
            # execute the plain loop once, rather than entering an in-loop G
            # clone per iteration. Count the actual ordinary stores as well:
            # an empty/dead plain copy must not satisfy refusal coverage.
            loops = 2 if kind == "all" else 1
            require(counters.get("rloop_g", 0) == 0
                    and counters.get("rloop_plain", 0) == loops
                    and counters.get("rloop_guard", 0) == loops,
                    f"{label}: affected preheader refusal needs {loops} guarded plain loops: {counters}")
            require(counters.get("emit.cfield.ic_call", 0) == expected
                    and counters.get("emit.cfield.guard_fallback_call", 0) == expected,
                    f"{label}: affected ordinary stores must execute exactly {expected} times: {counters}")
    return counters


def isolated_refusal_formation(ir, kind):
    """Runtime zero F is meaningful only if the same executable emitted it."""
    require(not any(marker in ir for marker in (
        "class_field.loop.", "class_field_loop.", "class_field_loop_store.",
        "for.class_field_fast", "for.class_field_slow")), "isolated control retained legacy IR")
    if kind == "reader":
        require("versioned_index.loop.fast.preheader" in ir,
                "reader refusal control did not emit the positive indexed route")
        return {"indexed route formed but refused at runtime": True}
    name = "affectedPair" if kind == "pair" else "untouchedCell"
    bodies = [body for symbol, body in re.findall(
        r'^define[^\n]*@([^\s(]+)[^\n]*\{\n(.*?)^\}', ir, re.MULTILINE | re.DOTALL)
        if symbol.strip('"').startswith("perry_fn_") and symbol.strip('"').endswith("__" + name)]
    require(len(bodies) == 1, f"isolated {kind} needs one {name} function")
    body = bodies[0]
    fast = guarded_fast_body(body, "isolated " + kind)
    expected_bare = 3 if kind == "pair" else 1
    require(fast.count("@js_recv_route_note(i32 18)") == expected_bare
            and "@js_recv_route_note(i32 34)" in fast and "store double" in fast,
            f"isolated {kind} needs its own exact R/store F")
    if kind == "pair":
        require("load double" in fast and "fadd double" in fast,
                "affected Pair F must contain its own reads and addition")
    require(not any(marker in fast for marker in (
        "@js_object_get_field", "@js_put_value", "@js_gc_write_barrier",
        "@js_dynamic_string_or_number_add")), f"isolated {kind} retained ordinary F work")
    return {"isolated " + kind + " R/store route formed": True}


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--compiler", required=True, type=Path)
    ap.add_argument("--runtime-dir", required=True, type=Path)
    ap.add_argument("--node", required=True, type=Path)
    ap.add_argument("--build-receipt", required=True, type=Path)
    ap.add_argument("--out", required=True, type=Path)
    ap.add_argument("--llvm-opt", type=Path, help="LLVM 22 opt for production native-root rewriting")
    args = ap.parse_args()
    repo = Path(__file__).resolve().parents[1]
    head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=repo, text=True).strip()
    dirty = subprocess.check_output(
        ["git", "status", "--porcelain", "--untracked-files=normal"], cwd=repo, text=True
    )
    require(not dirty, "source must be frozen and clean before checking its build receipt")
    receipt = json.loads(args.build_receipt.read_text())
    require(receipt["commit"] == head, "build receipt is for another source head")
    paths = {"compiler": args.compiler.resolve(), "runtime": args.runtime_dir.resolve() / "libperry_runtime.a", "stdlib": args.runtime_dir.resolve() / "libperry_stdlib.a"}
    for kind, path in paths.items():
        require(sha(path) == receipt[f"{kind}_sha256"], f"{kind}: artifact hash differs from build receipt")
    require(not args.out.exists(), "output directory must be new (cold cache proof)")
    out = args.out.resolve()
    out.mkdir(parents=True)
    (out / "receipt.json").write_text(json.dumps(receipt, indent=2))
    guard = out / "tool-guards"
    guard.mkdir()
    cargo = guard / "cargo"
    cargo.write_text("#!/bin/sh\necho 'ConstFn gate forbids implicit Cargo builds' >&2\nexit 126\n")
    cargo.chmod(0o755)
    clean_env = {key: value for key, value in os.environ.items()
                 if not key.startswith("PERRY_") and key != "NODE_OPTIONS"}
    env = dict(clean_env, PATH=str(guard) + os.pathsep + os.environ["PATH"],
               PERRY_RUNTIME_DIR=str(args.runtime_dir.resolve()), PERRY_LIB_DIR=str(args.runtime_dir.resolve()),
               PERRY_DEV_VERBOSE="1", PERRY_STATIC_SHAPE_CENSUS="1", PERRY_METHOD_SITE_STATS="1",
               PERRY_GC_INSTRUMENTS="1")
    # Ambient trace overrides must not divert the LLVM receipt elsewhere.
    env.pop("PERRY_SAVE_LL", None)
    version = subprocess.check_output([str(args.node.resolve()), "--version"], text=True).strip()
    require(version == "v" + (repo / ".node-version").read_text().strip(), "Node oracle is not the exact repository pin")
    fixtures = repo / "test-files/constfn-executable-gates"
    summary = []

    def compile_fixture(name, arm, tag, trace=False, fail=False, regions=True):
        arm_env = dict(env, PERRY_CONSTFN_SHAPE=str(arm), PERRY_CACHE_DIR=str(out / f"cache-{name}-{arm}"))
        if not regions:
            arm_env.update(PERRY_REGIONS="0", PERRY_CACHE_DIR=str(out / f"cache-{name}-{arm}-regions-off"))
        if name.startswith("numeric-"):
            arm_env.update(PERRY_STORE_CENSUS="1", PERRY_RECV_ROUTE_COUNT="1")
        binary = out / f"{name}-{arm}-{tag}"
        command = [str(args.compiler.resolve()), "compile", str(fixtures / f"{name}.ts"), "--no-auto-optimize", "--debug-symbols", "-o", str(binary)]
        if trace:
            trace_dir = out / "ir" / name / str(arm) if name.startswith("numeric-refusal-") or name == "numeric-class-loop-replacement" else out / "ir" / name
            trace_dir.mkdir(parents=True)
            arm_env["PERRY_SAVE_LL"] = str(trace_dir)
            arm_env["PERRY_NO_CACHE"] = "1"
            arm_env["PERRY_INLINE_SHADOW_SLOT"] = "0"
            command += ["--trace", "llvm"]
        p = run(command, arm_env, out, out / f"compile-{name}-{arm}-{tag}")
        if not fail:
            good(p, f"compile {name}/{arm}/{tag}")
        return binary, p, arm_env

    refusal_controls = {
        "pair": [((), "401 200 200 200\n", True)],
        "reader": [((), "24384 1\n", True)],
        "cell": [((), "42\n", False), (("refuse",), "42 0 200\n", True)],
        "all": [((), "401 200 200 200\n42 0 200\n", True)],
    }
    for kind, cases in refusal_controls.items():
        name = "numeric-refusal-" + kind
        for arguments, expected, _ in cases:
            p = run([str(args.node.resolve()), "--experimental-strip-types",
                     str(fixtures / f"{name}.ts"), *arguments], env, out,
                    out / f"node-{name}-{'mutated' if arguments else 'default'}")
            good(p, "Node " + name)
            require(p.stdout == expected, f"{name}: exact descriptor/coercion events changed")
        for arm in (0, 1):
            for regions in (False, True):
                tag = "regions-on" if regions else "regions-off"
                binary, _, arm_env = compile_fixture(name, arm, tag, trace=regions, regions=regions)
                for arguments, expected, mutated in cases:
                    log = out / f"run-{name}-{arm}-{tag}-{'mutated' if arguments else 'default'}"
                    p = run([str(binary), *arguments], arm_env, out, log)
                    good(p, name + "/" + tag)
                    require(p.stdout == expected, f"{name}/{arm}/{tag}: exact events differ from Node")
                    if regions:
                        routes = isolated_refusal_routes(p.stderr, str(log), kind=kind, mutated=mutated)
                        log.with_suffix(".routes.json").write_text(json.dumps(routes, indent=2))
            if kind != "all":
                ir = "\n".join(p.read_text() for p in (out / "ir" / name / str(arm)).rglob("*.ll"))
                isolated_refusal_formation(ir, kind)
            summary.append([name, arm, "affected receiver events and on/off attribution PASS", []])

    collecting_expected = {
        "collecting-nested-add": "".join(f"captured-{i}5\n" for i in range(12)) + "12 changed 127\n",
        "collecting-field-assignment": "66 11 12 127\n",
    }
    for name, expected in collecting_expected.items():
        node = run([str(args.node.resolve()), "--expose-gc", "--experimental-strip-types",
                    str(fixtures / f"{name}.ts")], env, out, out / ("node-" + name))
        good(node, "Node " + name)
        require(node.stdout == expected, name + ": pinned coercion/setter oracle changed")
        for arm in (0, 1):
            binary, _, arm_env = compile_fixture(name, arm, "cold")
            results = []
            for seed in (1, 17, 991):
                moving = dict(arm_env, PERRY_GC_SCHEDULE_SEED=str(seed), PERRY_GC_SCHEDULE_RATE="0.25",
                              PERRY_GC_SCHEDULE_ALLOC_KB="0", PERRY_GC_FORCE_EVACUATE="1",
                              PERRY_GC_VERIFY_EVACUATION="1", PERRY_GC_PROTECT_FROMSPACE="1", PERRY_GC_DIAG="1")
                p = run([str(binary)], moving, out, out / f"run-{name}-{arm}-{seed}")
                good(p, f"{name}/{arm}/seed{seed}")
                require(p.stdout == expected, f"{name}/{arm}/seed{seed}: captured value/result mismatch")
                counters = re.search(r"\[gc-schedule\] done:.*copying_minors=(\d+) moved_objects=(\d+)", p.stderr)
                require(counters and min(map(int, counters.groups())) > 0,
                        f"{name}/{arm}/seed{seed}: collecting probe moved no objects")
                results.append(tuple(map(int, counters.groups())))
            if arm:
                compile_fixture(name, arm, "ir", trace=True)
            summary.append([name, arm, "3 moving coercion/setter seeds PASS", results])

    for name, expected in EXPECTED.items():
        if not name.startswith("workers"):
            p = run([str(args.node.resolve()), "--experimental-strip-types", str(fixtures / f"{name}.ts")], env, out, out / f"node-{name}")
            good(p, f"Node {name}")
            require(p.stdout == expected, f"{name}: expected oracle output changed")
        if name == "numeric-recheck":
            p = run([str(args.node.resolve()), "--experimental-strip-types",
                     str(fixtures / "numeric-recheck.ts"), "mutate"], env, out, out / "node-recheck-mutated")
            good(p, "Node recheck descriptor mutation")
            require(p.stdout == "1600 200 1536 200\n", "recheck mutation changed the oracle result")
        for arm in (0, 1):
            binary, compiler, arm_env = compile_fixture(name, arm, "cold")
            results = []
            for seed in (1, 17, 991):
                moving = dict(arm_env, PERRY_GC_SCHEDULE_SEED=str(seed), PERRY_GC_SCHEDULE_RATE="0.25",
                              PERRY_GC_SCHEDULE_ALLOC_KB="0", PERRY_GC_FORCE_EVACUATE="1",
                              PERRY_GC_VERIFY_EVACUATION="1", PERRY_GC_PROTECT_FROMSPACE="1", PERRY_GC_DIAG="1")
                p = run([str(binary)], moving, out, out / f"run-{name}-{arm}-{seed}")
                good(p, f"{name}/{arm}/seed{seed}")
                require(p.stdout == expected, f"{name}/{arm}/seed{seed}: output mismatch")
                if not name.startswith("workers"):
                    witnesses(p.stderr, bool(arm), name, 4096 if name in ("factory", "cache") else 1024)
                else:
                    census(p.stderr, name, constfn=bool(arm), worker=True)
                if name == "numeric-loops":
                    routes = numeric_routes(p.stderr, f"numeric/{arm}/seed{seed}", constfn=bool(arm))
                    (out / f"numeric-routes-{arm}-{seed}.json").write_text(json.dumps(routes, indent=2))
                counters = re.search(r"\[gc-schedule\] done:.*copying_minors=(\d+) moved_objects=(\d+)", p.stderr)
                require(counters and min(map(int, counters.groups())) > 0, f"{name}/{arm}: GC stress moved no objects")
                if name == "numeric-recheck":
                    routes = recheck_routes(p.stderr, f"recheck/{arm}/seed{seed}")
                    (out / f"recheck-routes-{arm}-{seed}.json").write_text(json.dumps(routes, indent=2))
                    changed = run([str(binary), "mutate"], moving, out,
                                  out / f"run-recheck-mutated-{arm}-{seed}")
                    good(changed, f"recheck mutation/{arm}/seed{seed}")
                    require(changed.stdout == "1600 200 1536 200\n",
                            f"recheck mutation/{arm}/seed{seed}: output mismatch")
                    routes = recheck_routes(changed.stderr, f"recheck mutation/{arm}/seed{seed}", mutated=True)
                    (out / f"recheck-mutated-routes-{arm}-{seed}.json").write_text(json.dumps(routes, indent=2))
                    moved = re.search(r"\[gc-schedule\] done:.*copying_minors=(\d+) moved_objects=(\d+)", changed.stderr)
                    require(moved and min(map(int, moved.groups())) > 0,
                            f"recheck mutation/{arm}/seed{seed}: GC stress moved no objects")
                results.append(tuple(map(int, counters.groups())))
            summary.append([name, arm, "3 moving seeds PASS", results])
            if name == "numeric-loops":
                p = run([str(args.node.resolve()), "--experimental-strip-types",
                         str(fixtures / "numeric-loops.ts"), "refuse"], env, out,
                        out / f"node-numeric-refuse-{arm}")
                good(p, "Node numeric descriptor refusal")
                require(p.stdout == expected, "numeric refusal changed the oracle result")
                p = run([str(binary), "refuse"], arm_env, out, out / f"run-numeric-refuse-{arm}")
                good(p, "numeric descriptor refusal")
                require(p.stdout == expected, "numeric refusal output mismatch")
                routes = numeric_routes(p.stderr, f"numeric/{arm}/refuse",
                                        constfn=bool(arm), refusal=True)
                (out / f"numeric-routes-{arm}-refuse.json").write_text(json.dumps(routes, indent=2))
                summary.append(["numeric descriptor refusal", arm, "generic route PASS", routes])
            if name == "cache":
                cold = CACHE.search(compiler.stderr)
                require(cold and int(cold[1]) == 0 and int(cold[3]) > 0, "cold link used preexisting cached objects")
                cold_seeds = {str(p.relative_to(out)): p.read_bytes() for p in (out / "cache-cache-1").rglob("*.seeds")} if arm else {}
                warm, compiler_warm, _ = compile_fixture(name, arm, "warm")
                cache = CACHE.search(compiler_warm.stderr)
                require(cache and int(cache[1]) > 0 and int(cache[3]) == 0, "warm link did not reuse actual objects")
                p = run([str(warm)], arm_env, out, out / f"run-cache-{arm}-warm")
                good(p, "cached executable")
                require(p.stdout == expected, "warm executable output mismatch")
                witnesses(p.stderr, bool(arm), "warm", 4096)
                if arm:
                    require(cold_seeds == {str(p.relative_to(out)): p.read_bytes() for p in (out / "cache-cache-1").rglob("*.seeds")}, "cold/warm sidecar bytes differ")
                    cold_run = (out / "run-cache-1-1.stderr").read_text()
                    require(witnesses(cold_run, True, "cold") == witnesses(p.stderr, True, "warm"), "cold/warm linked static census differs")
                    # A valid-format body/rep lie must fail the real cached
                    # link or executable. A corrupt format must fail compilation.
                    seed_files = list((out / "cache-cache-1").rglob("*.seeds"))
                    lines = [(f, i, l.split()) for f in seed_files for i, l in enumerate(f.read_text().splitlines()) if len(l.split()) == 6 and l.split()[5] != "-"]
                    bodies = sorted({e.split("@", 1)[1] for _, _, fields in lines for e in fields[5].split(",")})
                    require(len(bodies) > 1, "body sabotage needs two real linked body symbols")
                    target, index, fields = lines[0]
                    original = target.read_text()
                    for kind in ("body", "rep", "format"):
                        altered = fields.copy()
                        if kind == "body":
                            slot, body = altered[5].split(",")[0].split("@")
                            altered[5] = f"{slot}@{next(b for b in bodies if b != body)}"
                        elif kind == "rep":
                            cf_slots = {int(e.split("@")[0]) for e in altered[5].split(",")}
                            slot = next(i for i in range(int(altered[1])) if i not in cf_slots)
                            altered[4] = hex(int(altered[4], 16) ^ (1 << (2 * slot)))
                        else:
                            altered = ["invalid-sidecar"]
                        changed = original.splitlines()
                        changed[index] = " ".join(altered)
                        target.write_text("\n".join(changed) + "\n")
                        try:
                            bad, cp, _ = compile_fixture(name, arm, f"sabotage-{kind}", fail=True)
                            if cp.returncode == 0:
                                cache = CACHE.search(cp.stderr)
                                require(cache and int(cache[1]) > 0 and int(cache[3]) == 0, f"{kind}: sabotage was hidden by recompilation")
                                rp = run([str(bad)], arm_env, out, out / f"run-sabotage-{kind}")
                                require(rp.returncode != 0, f"{kind}: linked seed mismatch did not fail")
                            else:
                                require("sidecar" in cp.stderr or "seed" in cp.stderr, f"{kind}: compilation failed for unrelated reason")
                            summary.append(["sidecar " + kind, 1, "detector failed planted cached link", []])
                        finally:
                            target.write_text(original)
            # A separate trace compile deliberately bypasses the object cache.
            if arm:
                _, _, _ = compile_fixture(name, arm, "ir", trace=True)
    # Attribute replacement to one executed increment function. The combined
    # fixture's other read regions cannot supply this witness.
    replacement = "numeric-class-loop-replacement"
    p = run([str(args.node.resolve()), "--experimental-strip-types",
             str(fixtures / f"{replacement}.ts")], env, out, out / "node-class-replacement")
    good(p, "Node class replacement")
    require(p.stdout == "200\n", "class replacement oracle changed")
    for arm in (0, 1):
        binary, _, arm_env = compile_fixture(replacement, arm, "cold")
        for seed in (1, 17, 991):
            moving = dict(arm_env, PERRY_GC_SCHEDULE_SEED=str(seed), PERRY_GC_SCHEDULE_RATE="0.25",
                          PERRY_GC_SCHEDULE_ALLOC_KB="0", PERRY_GC_FORCE_EVACUATE="1",
                          PERRY_GC_VERIFY_EVACUATION="1", PERRY_GC_PROTECT_FROMSPACE="1", PERRY_GC_DIAG="1")
            p = run([str(binary)], moving, out, out / f"run-class-replacement-{arm}-{seed}")
            good(p, f"class replacement/{arm}/seed{seed}")
            require(p.stdout == "200\n", "class replacement result mismatch")
            routes = class_store_routes(p.stderr, f"class replacement/{arm}/seed{seed}")
            moved = re.search(r"\[gc-schedule\] done:.*copying_minors=(\d+) moved_objects=(\d+)", p.stderr)
            require(moved and min(map(int, moved.groups())) > 0,
                    "class replacement stress moved no objects; retain failed coverage")
            (out / f"class-replacement-routes-{arm}-{seed}.json").write_text(json.dumps(routes, indent=2))
        compile_fixture(replacement, arm, "ir", trace=True)
        replacement_ir = "\n".join(p.read_text() for p in (out / "ir" / replacement / str(arm)).rglob("*.ll"))
        class_store_region_formation(replacement_ir)
        summary.append(["class replacement", arm, "isolated R/store and 3 moving seeds PASS", []])
    trace = out / "ir"
    require(list(trace.rglob("*.ll")), "no emitted LLVM to inspect")
    numeric_ir = "\n".join(p.read_text() for p in (trace / "numeric-loops").rglob("*.ll"))
    formation = {name: marker in numeric_ir for name, marker in (
        ("arithmetic loop region", "rloop.guard."),
        ("versioned indexed", "versioned_index.loop.fast.preheader"),
    )}
    formation["legacy class numeric IR absent"] = not any(marker in numeric_ir for marker in
        ("class_field.loop.", "class_field_loop.", "class_field_loop_store.",
         "for.class_field_fast", "for.class_field_slow"))
    formation.update(arithmetic_region_formation(numeric_ir))
    (out / "numeric-formation.json").write_text(json.dumps(formation, indent=2))
    (out / "numeric-campaign-scope.json").write_text(json.dumps({
        "constfn_numeric_route_interop": True,
        "p7_arithmetic_or_f_rep_verified": True,
        "p8_isolated_class_R_store_verified": True,
        "p8_full_deletion_accepted": False,
        "performance_measured": False,
        "dependency": "P7 arithmetic twins and negative IR unit tests, sabotage, and performance gates remain separately required.",
    }, indent=2))
    require(all(formation.values()), "numeric fixture did not form every required guard tier; inspect IR and repair the fixture before acceptance")
    checker = repo / "scripts/gc_root_dominance_check.py"
    raw_ir = sorted(trace.rglob("*.ll"))
    if any('gc "statepoint-example"' in path.read_text() for path in raw_ir):
        # --trace llvm is pre-RS4GC. Analyze the actual production rewrite;
        # shadow-only passes cannot establish native-root correctness.
        from read_statepoint_rewrite_passes import find_declaration
        passes_source, passes = find_declaration()
        opt = args.llvm_opt or (Path(shutil.which("opt")) if shutil.which("opt") else None)
        require(opt is not None and opt.is_file(), "native root gate needs LLVM 22 opt; supply --llvm-opt")
        version = run([str(opt.resolve()), "--version"], env, repo, out / "llvm-opt-version")
        good(version, "LLVM opt version")
        require(re.search(r"LLVM version 22\.", version.stdout), "root rewrite must use LLVM 22")
        native = out / "native-ir"
        native.mkdir()
        rewrites = []
        for index, path in enumerate(raw_ir):
            target = native / path.relative_to(trace)
            target.parent.mkdir(parents=True, exist_ok=True)
            command = [str(opt.resolve()), "-passes=" + passes, "-S", str(path), "-o", str(target)]
            p = run(command, env, repo, out / f"root-rewrite-{index}")
            good(p, f"production root rewrite {path.relative_to(trace)}")
            rewrites.append({"input": str(path.relative_to(out)), "input_sha256": sha(path),
                             "output": str(target.relative_to(out)), "output_sha256": sha(target),
                             "command": command})
        (out / "root-rewrite-receipt.json").write_text(json.dumps({
            "commit": head, "passes_source": str(passes_source.relative_to(repo)),
            "passes_source_sha256": sha(passes_source), "passes": passes,
            "opt": str(opt.resolve()), "opt_sha256": sha(opt.resolve()), "modules": rewrites,
        }, indent=2))
        p = run([sys.executable, str(checker), "--statepoints", "--min-funcs", "30",
                 "--min-statepoints", "100", "--min-live-bundles", "50", "--min-relocates", "100",
                 str(native)], env, repo, out / "root-statepoints")
        good(p, "full rewritten native root correctness")
    else:
        # The fallback trace explicitly disables inline shadow bindings so
        # the checker can see real root stores. Its two modes are exclusive.
        for mode in ("stale-registers", "unrooted-allocas"):
            p = run([sys.executable, str(checker), "--" + mode, str(trace)], env, repo,
                    out / ("root-" + mode))
            good(p, "shadow root " + mode)
    (out / "summary.json").write_text(json.dumps(summary, indent=2))
    print(json.dumps(summary, indent=2))


if __name__ == "__main__":
    try:
        main()
    except (RuntimeError, OSError, KeyError) as error:
        print(f"ConstFn executable gate FAILED: {error}", file=sys.stderr)
        sys.exit(1)
