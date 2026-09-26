#!/usr/bin/env python3
"""Call-chain attribution for the package-performance harness (Phase 3).

Invoked as `scripts/package_bench.py profile --callgraph ...` (the flat
top-N `profile` stays as it was). For every selected workload this:

  1. measures Node and Perry instructions/iteration with the harness's
     two-N method (`perf stat -e instructions:u` at n1 and n2), checking
     Perry's stdout against Node's byte-for-byte -- a MISMATCH is recorded
     and not attributed;
  2. `perf record`s the Perry binary at n1 AND n2 with a FIXED sample period
     on `instructions:u` and DWARF call chains (`--call-graph dwarf`; perf's
     LBR mode is Intel-only and Perry's auto-optimized runtime is rebuilt
     without frame pointers, so frame-pointer chains break inside it);
  3. attributes each sample (weight = period) to its leaf symbol (self), to
     every distinct symbol on its chain (inclusive), to a root-cause BUCKET,
     to the RUNTIME ENTRY point (the runtime function that generated code
     called) and to the JS SITE (the innermost generated-code frame, resolved
     to file:line when the binary was compiled with `--debug-symbols`);
  4. subtracts the n1 profile from the n2 profile per key and divides by
     (n2 - n1) -- the same two-N method as the instruction counts, so module
     init, compilation of the package and warm-up cancel and every number is
     instructions PER ITERATION.

Outputs (merged into existing files, so a `--filter` re-run refreshes only
its workloads): `<out>.json` (everything) and `<out>.md` (report: ranked
buckets weighted by each workload's excess over Node, package x bucket
matrix, per-workload top chains).

Buckets are assigned by walking the chain from the leaf towards the root and
taking the FIRST frame that matches a rule in BUCKET_RULES (generic helpers
such as memcpy/memcmp have no rule, so they are charged to the runtime
function that called them). Two rules pre-empt the walk: a GC collection
anywhere on the chain charges the sample to gc_* (a collection is caused by
allocation volume, not by the construct that happened to trip it), and a
leaf in generated code charges `generated_code`.

Requirements: Linux perf with DWARF unwinding, `nm`, and (for file:line)
`llvm-addr2line` or binutils `addr2line`. Compile binaries with
PERRY_KEEP_SYMBOLS=1 (and ideally `--perry-flags=--debug-symbols`).
"""

from __future__ import annotations

import json
import os
import re
import shutil
import subprocess
import tempfile
from collections import Counter, defaultdict
from pathlib import Path

# ---------------------------------------------------------------- symbols

# Generated-code symbols (perry-codegen): plain C-ABI names with these
# prefixes. Everything else in the binary is runtime/stdlib/libc.
GEN_PREFIXES = ("perry_fn_", "perry_closure_", "perry_method_", "perry_class_", "__perry_", "perry_module_",
                "perry_init", "perry_static_", "perry_getter_", "perry_setter_", "perry_ctor_")
GEN_EXACT = {"main"}


def is_generated(sym: str) -> bool:
    return sym in GEN_EXACT or sym.startswith(GEN_PREFIXES)


def clean_sym(sym: str) -> str:
    """Strip perf's hash suffixes / generic args so symbols aggregate."""
    s = re.sub(r"::h[0-9a-f]{16}$", "", sym)
    s = re.sub(r"\+0x[0-9a-f]+$", "", s)
    return s


# Ordered (bucket, regex) rules, matched against a cleaned symbol. The first
# frame (walking leaf -> root) that matches ANY rule decides the bucket; the
# first matching rule for that frame wins.
GC_COLLECT = [
    ("gc_minor", r"gc::.*(minor|copying|evacuat|nursery|scaveng|promot)|js_gc_minor|copying_reset|forward_slot"),
    ("gc_major", r"gc::.*(major|full|mark|sweep|trace|defrag|old_gen)|js_gc_collect|gc_collect|mark_sweep"),
]
BUCKET_RULES = [
    ("write_barrier", r"write_barrier|remember(ed)?_set|card_mark|js_gc_barrier|barrier::"),
    ("gc_other", r"^perry_runtime::gc::|gc_safepoint_moving|layout_note_slot|gc::layout"),
    ("call_overhead", r"safepoint|shadow_(frame|slot|stack)|js_param_type_guard|stack_check|js_gc_loop_safepoint"
                      r"|frame_push|frame_pop|js_call_function|call_closure|js_closure_call|native_call_value"
                      r"|apply_function|invoke_closure"),
    ("regex", r"perex|regex|RegExp|js_regexp"),
    ("json", r"json|JSON"),
    ("bigint", r"bigint|BigInt"),
    ("promise_async", r"promise|microtask|async_|generator|await|js_task|event_loop|timer"),
    ("exceptions", r"exception::|throw|js_try_|try_push|landing|unwind|_Unwind|error::"),
    ("private_fields", r"private|brand"),
    ("accessor_descriptor", r"descriptor_state|accessor|getter|setter|define_property|property_descriptor"
                            r"|class_accessor_cache|js_object_define"),
    ("method_dispatch", r"native_call_method|call_method|method_dispatch|handle_method|dispatch_method"
                        r"|js_native_call|bound_method|method_cache|resolve_method"),
    ("buffer_typedarray", r"is_registered_buffer|is_uint8array_buffer|buffer::|buffer_data|uint8array|typed_array"
                          r"|typedarray|js_buffer_|dataview|array_buffer"),
    ("dyn_index", r"js_dyn_index|dyn_index|array::indexing|js_array_get|js_array_set|array::named_props"
                  r"|element_shape|js_array_|array::|holes"),
    ("map_set", r"js_map_|js_set_|::map::|::set::|weakmap|weak_map|collections::"),
    ("number_string", r"number_to_string|num_to_str|dtoa|ryu|fmt::float|parse_float|parse_int|js_number_to_|to_fixed"
                      r"|float_to|f64_to_str|js_parse"),
    ("string", r"string::|js_string|from_utf8|utf16|utf8|char_ops|intern|str::|concat|js_str_|to_lower|to_upper"
               r"|string_builder|rope|substring|char_code"),
    ("numeric_conv", r"fmod|trunc|to_int32|to_uint32|js_dynamic_bit|js_math|libm|floor|ceil|round|pow|js_number"),
    ("closure_box_args", r"closure::|js_closure|box::|js_box_|capture|arguments|js_make_closure|bound_function"),
    ("symbol", r"symbol::|js_symbol"),
    ("prop_write", r"set_field|js_object_set|field_set|add_transition|key_add|set_property|store_ic|write_ic"
                   r"|js_set_property|set_object_keys|put_field"),
    ("prop_lookup", r"shape_descriptor|keys_find_slot|keys_lookup|try_data_get|get_field_by|ic_miss|native_get"
                    r"|inherited_read|canonical_keys|shapes::|field_get|get_property|js_object_get|class_meta"
                    r"|prototype|proto_|class_registry|keys_|lookup|js_get_|object::|has_own|in_operator"
                    r"|native_module_registry|global_this"),
    ("side_table_hash", r"sip::Hasher|RandomState|hashbrown|HashMap|BTreeMap|hash::"),
    ("alloc", r"arena|alloc|mi_|malloc|free|calloc|realloc|js_object_new|object_new|js_new_"),
    ("value_typeof", r"typeof|js_is_truthy|js_jsvalue|value::|addr_class|nanbox|js_strict_eq|js_loose_eq|equals"
                     r"|js_compare|instanceof"),
]
_GC_RX = [(b, re.compile(r)) for b, r in GC_COLLECT]
_RULE_RX = [(b, re.compile(r)) for b, r in BUCKET_RULES]

BUCKET_LABELS = {
    "gc_minor": "GC: minor/copying collection",
    "gc_major": "GC: major (mark-sweep) collection",
    "gc_other": "GC: other (safepoint bookkeeping, layout notes)",
    "write_barrier": "GC write barrier",
    "call_overhead": "call overhead (safepoints, shadow frames, param guards, dynamic calls)",
    "regex": "regex (Perex)",
    "json": "JSON",
    "bigint": "BigInt",
    "promise_async": "Promise / async / timers",
    "exceptions": "exceptions / try-catch setup",
    "private_fields": "private class members",
    "accessor_descriptor": "accessor / property-descriptor handling",
    "method_dispatch": "runtime method dispatch (obj.m() via dispatcher)",
    "buffer_typedarray": "Buffer / typed-array access + buffer-registry probes",
    "dyn_index": "dynamic index get/set, Array element access",
    "map_set": "Map / Set",
    "number_string": "number <-> string",
    "string": "string ops / UTF-8<->UTF-16 transcoding",
    "numeric_conv": "numeric conversion (ToInt32, fmod, trunc, Math)",
    "closure_box_args": "closures / boxed captures / arguments",
    "symbol": "Symbol-keyed properties",
    "prop_write": "property write (add/overwrite slow path)",
    "prop_lookup": "property lookup slow path / IC miss",
    "side_table_hash": "std HashMap/SipHash side tables",
    "alloc": "allocation (arena / malloc)",
    "value_typeof": "value tests (typeof/equality/truthiness)",
    "generated_code": "generated code (inline JS)",
    "other_runtime": "other runtime",
    "unknown": "unresolved frames",
}


def classify(frames: list[str]) -> str:
    """frames: cleaned symbols, leaf first."""
    if not frames:
        return "unknown"
    for f in frames:
        for b, rx in _GC_RX:
            if rx.search(f):
                return b
    if is_generated(frames[0]):
        return "generated_code"
    for f in frames:
        if is_generated(f):
            break
        for b, rx in _RULE_RX:
            if rx.search(f):
                return b
    if frames[0].startswith("0x") or frames[0] == "[unknown]":
        return "unknown"
    return "other_runtime"


# ---------------------------------------------------------------- perf

def perf_stat(cmd, env, cwd, timeout):
    with tempfile.NamedTemporaryFile(suffix=".stat", delete=False) as f:
        path = f.name
    try:
        p = subprocess.run(["perf", "stat", "-x", ",", "-e", "instructions:u", "-o", path, "--"] + cmd,
                           cwd=cwd, env=env, capture_output=True, text=True, timeout=timeout)
        instr = None
        for line in Path(path).read_text().splitlines():
            parts = line.split(",")
            if len(parts) > 2 and parts[2].startswith("instructions"):
                try:
                    instr = int(parts[0])
                except ValueError:
                    pass
        return p.returncode, p.stdout, p.stderr, instr
    except subprocess.TimeoutExpired:
        return -999, "", "TIMEOUT", None
    finally:
        os.unlink(path)


def perf_record(cmd, env, cwd, timeout, period, stack, data, mode):
    cg = f"dwarf,{stack}" if mode == "dwarf" else mode
    p = subprocess.run(["perf", "record", "-q", "-e", "instructions:u", "-c", str(period), "--call-graph", cg,
                        "-o", str(data), "--"] + cmd, cwd=cwd, env=env, capture_output=True, text=True,
                       timeout=timeout)
    return p.returncode, p.stdout


_FRAME_RX = re.compile(r"^\s*([0-9a-f]+)\s+(.*?)\s+\((.*)\)\s*$")


def iter_samples(data: Path, binary: Path):
    """Yield (period, [(sym, symoff, dso), ...leaf first]) per sample."""
    p = subprocess.Popen(["perf", "script", "-i", str(data), "-F", "period,ip,sym,symoff,dso", "--no-inline"],
                         stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True, errors="replace")
    period, frames = None, []
    for line in p.stdout:
        if not line.strip():
            if period is not None:
                yield period, frames
            period, frames = None, []
            continue
        if period is None:
            m = re.match(r"^\s*(\d+)\s*$", line) or re.match(r"^\s*(\d+)\s", line)
            if m and not _FRAME_RX.match(line):
                period = int(m.group(1))
                continue
            # perf puts the period on the first line; tolerate odd layouts
            period = 1
        m = _FRAME_RX.match(line)
        if m:
            symoff, dso = m.group(2), m.group(3)
            mo = re.match(r"^(.*)\+0x([0-9a-f]+)$", symoff)
            sym, off = (mo.group(1), int(mo.group(2), 16)) if mo else (symoff, 0)
            frames.append((sym, off, dso))
    if period is not None:
        yield period, frames
    p.wait()


def nm_addrs(binary: Path) -> dict:
    out = subprocess.run(["nm", "--defined-only", str(binary)], capture_output=True, text=True).stdout
    addrs = {}
    for line in out.splitlines():
        parts = line.split()
        if len(parts) == 3 and is_generated(parts[2]):
            addrs.setdefault(parts[2], int(parts[0], 16))
    return addrs


def addr2line(binary: Path, addrs: list[int]) -> dict:
    tool = shutil.which("llvm-addr2line") or next(
        (str(p) for p in sorted(Path("/usr/lib").glob("llvm-*/bin/llvm-addr2line"), reverse=True)), None) \
        or shutil.which("addr2line")
    if not tool or not addrs:
        return {}
    inp = "\n".join(hex(a) for a in addrs) + "\n"
    p = subprocess.run([tool, "-e", str(binary), "-f", "-C"], input=inp, capture_output=True, text=True)
    lines = p.stdout.splitlines()
    res = {}
    for i, a in enumerate(addrs):
        if 2 * i + 1 < len(lines):
            loc = lines[2 * i + 1]
            res[a] = None if loc.startswith("??") else re.sub(r" \(discriminator \d+\)", "", loc)
    return res


def short_loc(loc: str | None) -> str | None:
    if not loc:
        return None
    m = re.search(r"node_modules/(.*)$", loc)
    if m:
        return m.group(1)
    m = re.search(r"benchmarks/packages/(.*)$", loc)
    return m.group(1) if m else loc


def aggregate(data: Path, binary: Path, gen_addr: dict) -> dict:
    """Weighted counters over one perf.data. Keys are strings."""
    self_c, incl_c, bucket_c, entry_c, site_c, chain_c = (Counter() for _ in range(6))
    total = 0
    n = 0
    reached_main = 0
    for period, frames in iter_samples(data, binary):
        n += 1
        total += period
        syms = [clean_sym(s) for s, _o, _d in frames]
        if any(s == "main" or s.startswith("__libc_start") for s in syms):
            reached_main += 1
        leaf = syms[0] if syms else "[unknown]"
        self_c[leaf] += period
        for s in set(syms):
            incl_c[s] += period
        b = classify(syms)
        bucket_c[b] += period
        # runtime entry + innermost generated frame
        entry, site = None, None
        for i, s in enumerate(syms):
            if is_generated(s):
                entry = syms[i - 1] if i > 0 else "<inline JS>"
                off = frames[i][1]
                base = gen_addr.get(s)
                # return address - 1 lies inside the call instruction
                site = (s, (base + off - (1 if i > 0 else 0)) if base is not None else None)
                break
        if entry is None:
            entry = syms[-1] if syms else "[unknown]"
            site = ("<no JS frame>", None)
        entry_c[entry] += period
        site_c[f"{site[0]}@{site[1] if site[1] is not None else ''}"] += period
        chain_c[f"{b}\t{leaf}\t{entry}\t{site[0]}@{site[1] if site[1] is not None else ''}"] += period
    return {"samples": n, "total": total, "reached_main": reached_main, "self": self_c, "incl": incl_c,
            "bucket": bucket_c, "entry": entry_c, "site": site_c, "chain": chain_c}


def per_iter_diff(a2: Counter, a1: Counter, dn: int) -> dict:
    keys = set(a2) | set(a1)
    return {k: (a2.get(k, 0) - a1.get(k, 0)) / dn for k in keys}


def top(d: dict, k: int, total: float):
    items = sorted(d.items(), key=lambda kv: -kv[1])[:k]
    return [[name, round(v), round(100.0 * v / total, 2) if total else None] for name, v in items]


# ---------------------------------------------------------------- driver

def run(args, pb) -> None:
    """`pb` is the package_bench module (servers, manifest, lock helpers)."""
    manifest = pb.load_manifest()
    wls = [w for w in pb.select_workloads(manifest, args.filter)
           if not w["id"].startswith("control/") or args.include_control]
    out_json = Path(args.out)
    doc = json.loads(out_json.read_text()) if out_json.exists() else {"workloads": {}}
    doc.setdefault("workloads", {})
    doc.update(schema="pkg-bench-callgraph/1", perry_bin_dir=str(args.perry_bin_dir),
               perry_commit=args.perry_commit, callgraph=args.callgraph_mode, target_samples=args.target_samples)
    node_ref = {}
    if args.node_instr:
        for wid, rec in json.loads(Path(args.node_instr).read_text()).get("workloads", {}).items():
            pi = rec.get("arms", {}).get("node", {}).get("instr", {}).get("per_iter")
            out = rec.get("arms", {}).get("node", {}).get("correctness_output")
            if pi is not None:
                node_ref[wid] = (pi, out)
    pb.signal.signal(pb.signal.SIGINT, pb._on_signal)
    pb.signal.signal(pb.signal.SIGTERM, pb._on_signal)
    pb.atexit.register(pb.stop_servers)
    if args.lock:
        pb.lock_acquire(args.owner, args.lock_timeout)
    env_extra: dict = {}
    status: dict = {}
    for srv in sorted({s for w in wls for s in w.get("servers", [])}):
        Path(args.server_root).mkdir(parents=True, exist_ok=True)
        status[srv] = pb.STARTERS[srv](args, env_extra) or "ok"
        pb.log(f"server {srv}: {status[srv]}")
    env = pb.run_env(env_extra)
    tmp = Path(args.tmp_dir or tempfile.gettempdir())
    tmp.mkdir(parents=True, exist_ok=True)

    def save():
        out_json.write_text(json.dumps(doc, indent=1, sort_keys=True))
        write_markdown(doc, out_json.with_suffix(".md"))

    for w in wls:
        wid, n1, n2, warm = w["id"], w["n1"], w["n2"], w["warm"]
        rec = {"n1": n1, "n2": n2, "warm": warm}
        doc["workloads"][wid] = rec
        bad = [s for s in w.get("servers", []) if status.get(s) != "ok"]
        if bad:
            rec.update(status="SKIP", reason=f"server(s) unavailable: {bad}")
            save()
            continue
        binary = Path(args.perry_bin_dir) / pb.bin_name(wid)
        if not binary.exists():
            rec.update(status="SKIP", reason="no binary")
            save()
            continue
        src = str(pb.PKG_DIR / f"{wid}.ts")
        # Node reference: per-iteration instructions + expected stdout.
        if wid in node_ref:
            node_pi, _ = node_ref[wid]
            node_out = {}
        else:
            node_pi, node_out = None, {}
        if node_pi is None or args.node_verify:
            i_node = {}
            for n in (n1, n2):
                rc, o, e, ins = perf_stat([args.node, "--no-warnings", src, str(n), str(warm)], env, pb.PKG_DIR,
                                          args.timeout)
                i_node[n], node_out[n] = ins, (o if rc == 0 else None)
            if node_pi is None and None not in i_node.values():
                node_pi = (i_node[n2] - i_node[n1]) / (n2 - n1)
        rec["node_per_iter"] = node_pi
        # Perry: correctness + instructions
        i_p = {}
        for n in (n1, n2):
            rc, o, e, ins = perf_stat([str(binary), str(n), str(warm)], env, pb.PKG_DIR, args.timeout)
            i_p[n] = ins
            if rc != 0:
                rec.update(status="FAIL", reason=f"perry exit {rc} at n={n}: {pb.tail(e, 300)}")
                break
            exp = node_out.get(n)
            if exp is not None and o != exp:
                rec.update(status="MISMATCH", reason=pb.first_diff(exp, o))
                break
            if exp is None:
                rec["correctness"] = "not checked against node (reused node reference)"
        if rec.get("status") in ("FAIL", "MISMATCH"):
            pb.log(f"{wid}: {rec['status']} {rec['reason'][:160]}")
            save()
            continue
        perry_pi = (i_p[n2] - i_p[n1]) / (n2 - n1)
        rec.update(perry_per_iter=perry_pi, perry_instr_n1=i_p[n1], perry_instr_n2=i_p[n2],
                   startup_instr=i_p[n1] - n1 * perry_pi)
        if node_pi:
            rec["ratio"] = perry_pi / node_pi
            rec["excess_per_iter"] = perry_pi - node_pi
        period = max(10007, int(i_p[n2] / args.target_samples) | 1)
        rec["period"] = period
        aggs = {}
        gen_addr = nm_addrs(binary)
        if not gen_addr:
            rec.update(status="FAIL", reason="no generated-code symbols: compile with PERRY_KEEP_SYMBOLS=1")
            save()
            continue
        for n in (n1, n2):
            data = tmp / f"pkgattr-{os.getpid()}-{n}.data"
            rc, _ = perf_record([str(binary), str(n), str(warm)], env, pb.PKG_DIR, args.timeout, period,
                                args.dwarf_stack, data, args.callgraph_mode)
            aggs[n] = aggregate(data, binary, gen_addr)
            data.unlink(missing_ok=True)
        a1, a2 = aggs[n1], aggs[n2]
        dn = n2 - n1
        sampled_pi = (a2["total"] - a1["total"]) / dn
        rec["sampled_per_iter"] = sampled_pi
        rec["samples"] = {"n1": a1["samples"], "n2": a2["samples"]}
        rec["unwind_reached_main"] = round(a2["reached_main"] / max(1, a2["samples"]), 3)
        base = perry_pi if perry_pi > 0 else sampled_pi
        # scale sampled per-iter values so they sum to the measured per-iter
        scale = base / sampled_pi if sampled_pi > 0 else 1.0
        rec["sample_scale"] = round(scale, 4)

        def d(key):
            return {k: v * scale for k, v in per_iter_diff(a2[key], a1[key], dn).items()}

        buckets = d("bucket")
        rec["buckets"] = {k: round(v) for k, v in sorted(buckets.items(), key=lambda kv: -kv[1]) if abs(v) >= 0.5}
        rec["self_top"] = top(d("self"), args.top, base)
        rec["incl_top"] = top({k: v for k, v in d("incl").items() if not is_generated(k)}, args.top, base)
        rec["incl_js_top"] = top({k: v for k, v in d("incl").items() if is_generated(k) and k != "main"},
                                 args.top, base)
        rec["entry_top"] = top(d("entry"), args.top, base)
        chains = top(d("chain"), args.top_chains, base)
        # resolve JS sites to file:line
        addrs = sorted({int(c[0].split("\t")[3].split("@")[1]) for c in chains
                        if c[0].split("\t")[3].split("@")[1]})
        sites = top(d("site"), args.top, base)
        addrs = sorted(set(addrs) | {int(s[0].split("@")[1]) for s in sites if s[0].split("@")[1]})
        locs = addr2line(binary, addrs)
        rec["chains"] = []
        for key, v, pct in chains:
            b, leaf, entry, site = key.split("\t")
            fn, a = site.split("@")
            rec["chains"].append({"bucket": b, "leaf": leaf, "entry": entry, "js_fn": fn,
                                  "js_loc": short_loc(locs.get(int(a))) if a else None,
                                  "per_iter": v, "pct": pct})
        rec["js_sites"] = []
        for key, v, pct in sites:
            fn, a = key.split("@")
            rec["js_sites"].append({"js_fn": fn, "js_loc": short_loc(locs.get(int(a))) if a else None,
                                    "per_iter": v, "pct": pct})
        rec["status"] = "OK"
        pb.log(f"{wid}: perry {perry_pi:,.0f}/iter node {node_pi or 0:,.0f}/iter; unwind->main "
               f"{rec['unwind_reached_main']:.0%}; top buckets "
               + ", ".join(f"{k} {100 * v / base:.0f}%" for k, v in list(rec["buckets"].items())[:4]))
        save()
    if args.lock:
        pb.lock_release()
    pb.stop_servers()
    save()


# ---------------------------------------------------------------- report

def bucket_excess_shares(rec: dict) -> dict:
    """Fraction of the workload's EXCESS over Node attributed to each bucket.
    Perry's per-iteration instructions are attributed; Node's cost is not
    (it runs a JIT), so a bucket's share of excess = its Perry instructions /
    excess, and the shares are normalised to sum to 1 when Perry/Node is
    small enough that the raw sum would exceed it."""
    ex = rec.get("excess_per_iter")
    b = rec.get("buckets") or {}
    if not ex or ex <= 0 or not b:
        return {}
    raw = {k: max(0.0, v) / ex for k, v in b.items()}
    s = sum(raw.values())
    if s > 1.0:
        raw = {k: v / s for k, v in raw.items()}
    return raw


def pkg(wid: str) -> str:
    return wid.split("/", 1)[0]


def rank_buckets(doc: dict, min_ratio: float = 2.0):
    wl = {w: r for w, r in doc["workloads"].items() if r.get("status") == "OK" and not w.startswith("control/")
          and (r.get("ratio") or 0) >= min_ratio}
    # per package: mean of its workloads' shares
    per_pkg: dict = defaultdict(list)
    for w, r in wl.items():
        per_pkg[pkg(w)].append(bucket_excess_shares(r))
    pkg_share = {}
    for p, lst in per_pkg.items():
        keys = set().union(*lst) if lst else set()
        pkg_share[p] = {k: sum(x.get(k, 0) for x in lst) / len(lst) for k in keys}
    buckets = sorted(set().union(*pkg_share.values()) if pkg_share else set())
    npk = len(pkg_share) or 1
    abs_total = sum(r["excess_per_iter"] for r in wl.values())
    ranked = []
    for b in buckets:
        eq = sum(s.get(b, 0) for s in pkg_share.values()) / npk
        ab = sum(bucket_excess_shares(r).get(b, 0) * r["excess_per_iter"] for r in wl.values())
        m5 = sorted(p for p, s in pkg_share.items() if s.get(b, 0) >= 0.05)
        ranked.append({"bucket": b, "equal_weight_pct": 100 * eq, "abs_weight_pct": 100 * ab / abs_total
                       if abs_total else 0, "packages_ge5pct": m5})
    ranked.sort(key=lambda x: -x["equal_weight_pct"])
    return ranked, pkg_share, wl


def fmt_n(x):
    if x is None:
        return "—"
    x = float(x)
    if abs(x) >= 1e6:
        return f"{x / 1e6:.2f}M"
    if abs(x) >= 1e4:
        return f"{x / 1e3:.1f}k"
    return f"{x:.0f}"


def write_markdown(doc: dict, path: Path) -> None:
    ranked, pkg_share, wl = rank_buckets(doc)
    L = ["# Package profile: where Perry's instructions go", "",
         f"Perry commit `{doc.get('perry_commit') or '?'}`; call chains: `perf record -e instructions:u "
         f"--call-graph {doc.get('callgraph')}`; ~{doc.get('target_samples')} samples at n2; every figure is "
         "instructions **per iteration** by the two-N method (the n1 profile is subtracted from the n2 "
         "profile key by key). Generated by `scripts/package_bench.py profile --callgraph`.", "",
         "## Ranked root-cause buckets", "",
         "Share of Perry's **excess over Node** (perry − node instructions/iter). *Equal-weight*: mean over "
         "packages (each package counts once, its workloads averaged) — the headline. *Abs-weight*: summed "
         "absolute excess (dominated by the most expensive workloads, shown for transparency). Workloads "
         "with Perry/Node < 2× are excluded.", "",
         "| # | bucket | equal-weight % of excess | abs-weight % | packages where ≥5% |", "|---|---|---:|---:|---|"]
    for i, r in enumerate(ranked, 1):
        L.append(f"| {i} | {BUCKET_LABELS.get(r['bucket'], r['bucket'])} (`{r['bucket']}`) | "
                 f"{r['equal_weight_pct']:.1f} | {r['abs_weight_pct']:.1f} | {len(r['packages_ge5pct'])}: "
                 f"{', '.join(r['packages_ge5pct'])} |")
    top_b = [r["bucket"] for r in ranked[:12]]
    L += ["", "## Package × bucket matrix (% of the package's excess)", "",
          "| package | " + " | ".join(f"`{b}`" for b in top_b) + " |",
          "|---|" + "---:|" * len(top_b)]
    for p in sorted(pkg_share):
        L.append(f"| {p} | " + " | ".join(f"{100 * pkg_share[p].get(b, 0):.0f}" for b in top_b) + " |")
    L += ["", "## Workloads", "",
          "| workload | perry/iter | node/iter | ratio | unwind→main | top buckets (% of Perry) |",
          "|---|---:|---:|---:|---:|---|"]
    for w, r in sorted(doc["workloads"].items()):
        if r.get("status") != "OK":
            L.append(f"| {w} | {r.get('status')} | | | | {str(r.get('reason', ''))[:120].replace('|', '/')} |")
            continue
        base = r["perry_per_iter"] or 1
        tb = ", ".join(f"{k} {100 * v / base:.0f}%" for k, v in list(r["buckets"].items())[:4])
        L.append(f"| {w} | {fmt_n(r['perry_per_iter'])} | {fmt_n(r.get('node_per_iter'))} | "
                 f"{(r.get('ratio') or 0):.1f}× | {r.get('unwind_reached_main', 0):.0%} | {tb} |")
    for w, r in sorted(doc["workloads"].items(), key=lambda kv: -(kv[1].get("excess_per_iter") or 0)):
        if r.get("status") != "OK":
            continue
        L += ["", f"### {w} — {fmt_n(r['perry_per_iter'])} instr/iter ({(r.get('ratio') or 0):.1f}× Node)", "",
              "Top chains (bucket · leaf ← runtime entry ← JS site):", "",
              "| instr/iter | % | bucket | leaf (self) | runtime entry | JS function | JS source |",
              "|---:|---:|---|---|---|---|---|"]
        for c in r["chains"][:12]:
            L.append(f"| {fmt_n(c['per_iter'])} | {c['pct']} | {c['bucket']} | `{c['leaf'][:60]}` | "
                     f"`{c['entry'][:60]}` | `{c['js_fn'][:60]}` | {c['js_loc'] or '—'} |")
        L += ["", "Top inclusive runtime functions: " + "; ".join(
            f"`{n[:60]}` {p}%" for n, _v, p in r["incl_top"][:8])]
        L += ["", "Top self: " + "; ".join(f"`{n[:60]}` {p}%" for n, _v, p in r["self_top"][:8])]
    path.write_text("\n".join(L) + "\n")


def main():
    import argparse
    ap = argparse.ArgumentParser(description="re-render the Markdown from a --callgraph JSON")
    ap.add_argument("json")
    ap.add_argument("--md")
    a = ap.parse_args()
    doc = json.loads(Path(a.json).read_text())
    write_markdown(doc, Path(a.md) if a.md else Path(a.json).with_suffix(".md"))


if __name__ == "__main__":
    main()
