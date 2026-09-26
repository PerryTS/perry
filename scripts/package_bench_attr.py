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


STARTUP_RX = re.compile(r"^(_start|__libc_start|main$|start_thread|clone3?$|\[unknown\]|0x)")


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
    ("write_barrier", r"write_barrier|remember(ed)?_set|card_mark|js_gc_barrier|barrier::|barrier_store"),
    ("gc_other", r"^perry_runtime::gc::|gc_safepoint_moving|layout_note_slot|gc::layout"),
    ("call_overhead", r"safepoint|shadow_(frame|slot|stack)|js_param_type_guard|stack_check|js_gc_loop_safepoint"
                      r"|frame_push|frame_pop|js_call_function|call_closure|js_closure_call|native_call_value"
                      r"|apply_function|invoke_closure|js_call_value|call_dynamic|implicit_this|call_location"
                      r"|js_closure_call\d"),
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
    ("string", r"^perry_runtime::string::|js_string|char_ops|js_str_|to_lower|to_upper|string_builder|rope"
               r"|substring|char_code|template|js_concat"),
    ("numeric_conv", r"fmod|trunc|to_int32|to_uint32|js_dynamic_bit|js_math|libm|floor|ceil|round|pow|js_number"),
    ("closure_box_args", r"closure::|js_closure|box::|js_box_|capture|arguments|js_make_closure|bound_function"),
    ("symbol", r"symbol::|js_symbol"),
    ("prop_write", r"set_field|js_object_set|field_set|add_transition|key_add|set_property|store_ic|write_ic"
                   r"|js_set_property|set_object_keys|put_field|js_put_value"),
    ("alloc", r"js_object_alloc|js_array_alloc|alloc_class|js_new_object|object_new|js_object_new|js_alloc"),
    ("prop_lookup", r"shape_descriptor|keys_find_slot|keys_lookup|try_data_get|get_field_by|ic_miss|native_get"
                    r"|inherited_read|canonical_keys|shapes::|field_get|get_property|js_object_get|class_meta"
                    r"|prototype|proto_|class_registry|keys_|lookup|js_get_|object::|has_own|in_operator"
                    r"|native_module_registry|global_this"),
    ("value_typeof", r"typeof|js_is_truthy|js_jsvalue|value::|addr_class|nanbox|js_strict_eq|js_loose_eq|equals"
                     r"|js_compare|instanceof"),
]
# WEAK rules name generic MECHANISMS (transcoding, hashing, raw allocation)
# that are charged to whatever runtime function called them. They decide the
# cause bucket only when no strong rule matches anywhere between the leaf and
# the JS frame; they always decide the leaf-first MECHANISM view.
WEAK_RULES = [
    ("utf8_transcode", r"from_utf8|str::converts|utf8|utf16|core::str::|run_utf8_validation"),
    ("side_table_hash", r"sip::Hasher|RandomState|hashbrown|HashMap|BTreeMap|hash::|FxHash|ahash"),
    ("raw_alloc", r"arena|alloc::|malloc|free|calloc|realloc|^mi_|_mi_|RawVec|finish_grow|__rust_alloc"),
    ("memops", r"memcmp|memcpy|memmove|memset|bcmp|strlen"),
]
# SUBSYSTEM rules pre-empt the leaf-first walk: when ANY runtime frame
# between the leaf and the JS frame matches, everything beneath it is the cost
# of that construct (a private read's shape lookups are the private read's
# cost; a regex match's memcmp is the regex's).
SUBSYSTEM_RULES = [
    ("regex", r"perex|regex|js_regexp"),
    ("json", r"json::|js_json|JSON"),
    ("bigint", r"bigint|BigInt"),
    ("private_fields", r"private_member|private_evaluation|private_value|js_private_|private_brand|private_field"),
    ("proto_chain_read", r"prototype_property_value|ordinary_object_prototype_property|proto_chain_get"),
    ("closure_box_args", r"js_arguments_|arguments_bundle"),
]
_SUB_RX = [(b, re.compile(r)) for b, r in SUBSYSTEM_RULES]
_GC_RX = [(b, re.compile(r)) for b, r in GC_COLLECT]
_RULE_RX = [(b, re.compile(r)) for b, r in BUCKET_RULES]
_WEAK_RX = [(b, re.compile(r)) for b, r in WEAK_RULES]

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
    "prop_lookup": "property lookup slow path / IC miss (own/by-name)",
    "proto_chain_read": "reads resolved on the prototype chain (absent keys, inherited data)",
    "side_table_hash": "std HashMap/SipHash side tables (no stronger owner on the chain)",
    "alloc": "object/array allocation entry points",
    "raw_alloc": "raw allocation (arena/malloc, no stronger owner on the chain)",
    "utf8_transcode": "UTF-8 validation / transcoding (no stronger owner on the chain)",
    "memops": "memcmp/memcpy (no stronger owner on the chain)",
    "value_typeof": "value tests (typeof/equality/truthiness)",
    "generated_code": "generated code (inline JS)",
    "other_runtime": "other runtime",
    "unknown": "unresolved frames",
}


def _match(f: str, rules) -> str | None:
    for b, rx in rules:
        if rx.search(f):
            return b
    return None


def classify(frames: list[str]) -> tuple[str, str]:
    """frames: cleaned symbols, leaf first. Returns (cause, mechanism)."""
    if not frames:
        return "unknown", "unknown"
    for f in frames:
        for b, rx in _GC_RX:
            if rx.search(f):
                return b, b
    if is_generated(frames[0]):
        return "generated_code", "generated_code"
    rt = []
    for f in frames:
        if is_generated(f):
            break
        rt.append(f)
    sub = next((b for f in rt for b in [_match(f, _SUB_RX)] if b), None)
    mech = None
    for f in rt:
        mech = _match(f, _WEAK_RX) or _match(f, _RULE_RX)
        if mech:
            break
    cause = sub or next((b for f in rt for b in [_match(f, _RULE_RX)] if b), None)
    if cause is None:
        cause = next((b for f in rt for b in [_match(f, _WEAK_RX)] if b), None)
    if cause is None:
        cause = "unknown" if frames[0].startswith("0x") or frames[0] == "[unknown]" else "other_runtime"
    return cause, mech or cause


def _decode_member(m: str) -> str:
    """Inverse of perry-codegen's `sanitize_member`: `u_` + `_<hex>_` escapes."""
    if not m.startswith("u_"):
        return m
    return re.sub(r"_([0-9a-f]{1,6})_", lambda x: chr(int(x.group(1), 16)), m[2:])


def js_name(sym: str) -> str:
    """Readable form of a generated symbol: `perry_method_<module>__<Class>__<m>`
    becomes `<module>::<Class>.<m>` with private/escaped names decoded."""
    m = re.match(r"^(perry_(?:fn|closure|method|static|getter|setter)_)(.*)$", sym)
    if not m:
        return sym
    kind, rest = m.groups()
    base, sep, suffix = rest.partition("$")
    parts = base.split("__")
    # re-join escaped member groups split on "__" (`u__23_W` -> ["u", "23_W"])
    out: list[str] = []
    i = 0
    while i < len(parts):
        cur = parts[i]
        if cur == "u" and i + 1 < len(parts):
            cur = "u_"
            while i + 1 < len(parts) and re.match(r"^[0-9a-f]{1,6}_", parts[i + 1]):
                cur += "_" + parts[i + 1]
                i += 1
        out.append(_decode_member(cur))
        i += 1
    name = out[0] + ("::" + ".".join(out[1:]) if len(out) > 1 else "")
    return name + (sep + suffix if sep else "")


def sanitize(name: str) -> str:
    """Byte-identical to perry-codegen `helpers::sanitize` (module prefixes)."""
    s = "".join(c if (c.isascii() and c.isalnum()) or c == "_" else "_" for c in name)
    return "_" + s if s[:1].isdigit() else s


class SourceIndex:
    """Maps a generated symbol back to package source: module prefix ->
    file (reverse of `sanitize(module path)`), then the member/function
    definition inside it, then -- for minified files that ship a source map
    -- the ORIGINAL source file:line."""

    def __init__(self, root: Path):
        self.root = root
        self.by_prefix: dict = {}
        for base in [root] + [p for p in root.iterdir() if p.is_dir()]:
            pass
        for path in list(root.rglob("*.js")) + list(root.rglob("*.mjs")) + list(root.rglob("*.cjs")) \
                + [p for p in root.rglob("*.ts") if "node_modules" not in p.parts]:
            rel = path.relative_to(root).as_posix()
            self.by_prefix.setdefault(sanitize(rel), rel)
            self.by_prefix.setdefault(sanitize(rel.rsplit(".", 1)[0]), rel)
            if rel.startswith("node_modules/"):
                continue
            # workloads compile as e.g. `lru_cache_churn_ts` / `_lib_bench_ts`
            self.by_prefix.setdefault(sanitize(rel.replace("/", "_")), rel)
            self.by_prefix.setdefault(sanitize(rel.split("/", 1)[-1]), rel)
        self._text: dict = {}
        self._maps: dict = {}
        self._cache: dict = {}

    def lines_of(self, path: str) -> list:
        if path not in self._text:
            self._text[path] = Path(path).read_text(errors="replace")
        return self._text[path].splitlines()

    def file_of(self, sym: str):
        m = re.match(r"^perry_(?:fn|closure|method|static|getter|setter)_(.*)$", sym)
        if not m:
            return None, None
        rest = m.group(1).split("$")[0]
        parts = rest.split("__")
        for k in range(len(parts), 0, -1):
            pre = "__".join(parts[:k])
            for cand in (pre, pre.lstrip("_")):
                if cand in self.by_prefix:
                    return self.by_prefix[cand], "__".join(parts[k:])
        return None, None

    def text(self, rel: str) -> str:
        if rel not in self._text:
            try:
                self._text[rel] = (self.root / rel).read_text(errors="replace")
            except OSError:
                self._text[rel] = ""
        return self._text[rel]

    def locate(self, sym: str, line_hint: int | None = None) -> str | None:
        key = (sym, line_hint)
        if key in self._cache:
            return self._cache[key]
        rel, tail = self.file_of(sym)
        res = None
        if rel:
            text = self.text(rel)
            pos = None
            name = js_name(sym)
            member = name.split("::", 1)[1].split("$")[0].split(".")[-1] if "::" in name else ""
            if member and not member.isdigit() and text:
                esc = re.escape(member)
                for pat in (rf"(?<![\w$.]){esc}\s*\([^()]*\)\s*\{{", rf"function\s*\*?\s*{esc}\s*\(",
                            rf"(?<![\w$]){esc}\s*[:=]\s*(?:async\s*)?(?:function\b|\([^()]*\)\s*=>|[\w$]+\s*=>)",
                            rf"(?<![\w$]){esc}\s*\("):
                    mm = re.search(pat, text)
                    if mm:
                        pos = mm.start()
                        break
            if pos is not None:
                line = text.count("\n", 0, pos) + 1
                col = pos - (text.rfind("\n", 0, pos) + 1)
            elif line_hint:
                line, col = line_hint, None
            else:
                line, col = None, None
            res = rel.replace("node_modules/", "") + (f":{line}" if line else "")
            if line and col is not None and len(text.splitlines()[line - 1]) > 500:
                orig = self.sourcemap(rel, line, col)
                if orig:
                    res += f" (orig {orig})"
        self._cache[key] = res
        return res

    def sourcemap(self, rel: str, line: int, col: int) -> str | None:
        if rel not in self._maps:
            self._maps[rel] = None
            m = re.search(r"sourceMappingURL=(\S+)", self.text(rel)[-2000:])
            if m and not m.group(1).startswith("data:"):
                mp = (self.root / rel).parent / m.group(1)
                try:
                    self._maps[rel] = (mp.parent, json.loads(mp.read_text()))
                except (OSError, ValueError):
                    pass
        entry = self._maps[rel]
        if not entry:
            return None
        base, sm = entry
        lines = sm.get("mappings", "").split(";")
        if line - 1 >= len(lines):
            return None
        # decode VLQ state up to the target line
        chars = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"
        src = oline = 0
        best = None
        for li, segs in enumerate(lines[:line]):
            gcol = 0
            for seg in filter(None, segs.split(",")):
                vals, shift, v = [], 0, 0
                for ch in seg:
                    d = chars.index(ch)
                    v += (d & 31) << shift
                    if d & 32:
                        shift += 5
                    else:
                        vals.append(-(v >> 1) if v & 1 else v >> 1)
                        v, shift = 0, 0
                gcol += vals[0]
                if len(vals) >= 4:
                    src += vals[1]
                    oline += vals[2]
                if li == line - 1 and len(vals) >= 4 and gcol <= col:
                    best = (src, oline)
        if best is None:
            return None
        srcs = sm.get("sources", [])
        name = srcs[best[0]] if best[0] < len(srcs) else "?"
        snippet = ""
        content = sm.get("sourcesContent") or []
        if best[0] < len(content) and content[best[0]]:
            ls = content[best[0]].splitlines()
            if best[1] < len(ls):
                snippet = " `" + ls[best[1]].strip()[:48].replace("|", "/").replace("`", "'") + "`"
        return f"{Path(name).name}:{best[1] + 1}{snippet}"


# ---------------------------------------------------------------- perf

def perf_stat(cmd, env, cwd, timeout):
    """Returns (rc, stdout, stderr, instructions, wall_seconds)."""
    import time
    with tempfile.NamedTemporaryFile(suffix=".stat", delete=False) as f:
        path = f.name
    t0 = time.perf_counter()
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
        return p.returncode, p.stdout, p.stderr, instr, time.perf_counter() - t0
    except subprocess.TimeoutExpired:
        return -999, "", "TIMEOUT", None, None
    finally:
        os.unlink(path)


def perf_record(cmd, env, cwd, timeout, freq, stack, data, mode):
    # Frequency mode: the kernel adapts the period and records it in every
    # sample, so weights still sum to instructions; a fixed `-c` period that
    # is too small for a fast loop gets THROTTLED (samples silently lost).
    cg = f"dwarf,{stack}" if mode == "dwarf" else mode
    p = subprocess.run(["perf", "record", "-q", "-e", "instructions:u", "-F", str(freq), "--call-graph", cg,
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


SEP, JSSEP = "\x1f", "\x1e"


def aggregate(data: Path, binary: Path, gen_addr: dict) -> dict:
    """Weighted counters over one perf.data. `stacks` keeps, per sample, the
    runtime frames (leaf first) up to the innermost generated-code frame plus
    that frame's call-site address -- everything the bucket rules and the
    chain/site tables need, so they can be re-derived (`--reanalyze`)
    without re-profiling."""
    self_c, incl_c, stacks = Counter(), Counter(), Counter()
    total = n = reached_main = 0
    for period, frames in iter_samples(data, binary):
        n += 1
        total += period
        syms = [clean_sym(s) for s, _o, _d in frames]
        if any(s == "main" or s.startswith("__libc_start") for s in syms):
            reached_main += 1
        self_c[syms[0] if syms else "[unknown]"] += period
        for s in set(syms):
            incl_c[s] += period
        rt, site = [], "<no JS frame>@"
        for i, s in enumerate(syms):
            if is_generated(s):
                base = gen_addr.get(s)
                # return address - 1 lies inside the call instruction
                addr = (base + frames[i][1] - (1 if i > 0 else 0)) if base is not None else None
                site = f"{s}@{addr if addr is not None else ''}"
                break
            rt.append(s)
        else:
            rt = rt[:12]
        stacks[SEP.join(rt) + JSSEP + site] += period
    return {"samples": n, "total": total, "reached_main": reached_main, "self": self_c, "incl": incl_c,
            "stacks": stacks}


def derive(stacks: Counter) -> dict:
    bucket_c, mech_c, entry_c, site_c, chain_c = (Counter() for _ in range(5))
    for key, w in stacks.items():
        rtp, site = key.split(JSSEP)
        rt = rtp.split(SEP) if rtp else []
        fn = site.split("@")[0]
        syms = rt + ([fn] if fn != "<no JS frame>" else [])
        b, mech = classify(syms) if syms else ("unknown", "unknown")
        entry = rt[-1] if rt and fn != "<no JS frame>" else ("<inline JS>" if not rt else "<no JS frame>")
        leaf = syms[0] if syms else "[unknown]"
        bucket_c[b] += w
        mech_c[mech] += w
        entry_c[entry] += w
        site_c[site] += w
        chain_c[f"{b}\t{leaf}\t{entry}\t{site}"] += w
    return {"bucket": bucket_c, "mech": mech_c, "entry": entry_c, "site": site_c, "chain": chain_c}


def per_iter_diff(a2: Counter, a1: Counter, dn: int) -> dict:
    keys = set(a2) | set(a1)
    return {k: (a2.get(k, 0) - a1.get(k, 0)) / dn for k in keys}


def top(d: dict, k: int, total: float):
    items = sorted(d.items(), key=lambda kv: -kv[1])[:k]
    return [[name, round(v), round(100.0 * v / total, 2) if total else None] for name, v in items]


def save_stacks(path: Path, a1: dict, a2: dict) -> None:
    import gzip
    keep = ("samples", "total", "reached_main", "self", "incl", "stacks")
    with gzip.open(path, "wt") as f:
        json.dump({"n1": {k: a1[k] for k in keep}, "n2": {k: a2[k] for k in keep}}, f)


def load_stacks(path: Path):
    import gzip
    with gzip.open(path, "rt") as f:
        d = json.load(f)
    conv = lambda a: {k: (Counter(v) if isinstance(v, dict) else v) for k, v in a.items()}
    return conv(d["n1"]), conv(d["n2"])


def analyze(rec: dict, a1: dict, a2: dict, binary: Path, srcidx, args) -> None:
    """Fill rec's per-iteration tables from the n1/n2 aggregates."""
    n1, n2 = rec["n1"], rec["n2"]
    perry_pi = rec["perry_per_iter"]
    dn = n2 - n1
    a1 = dict(a1, **derive(a1["stacks"]))
    a2 = dict(a2, **derive(a2["stacks"]))
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
    k_top = max(args.top, 20)
    incl = d("incl")
    rec["self_top"] = top(d("self"), k_top, base)
    rec["incl_top"] = top({k: v for k, v in incl.items() if not is_generated(k) and not STARTUP_RX.search(k)},
                          k_top, base)
    rec["incl_js_top"] = top({js_name(k): v for k, v in incl.items() if is_generated(k) and k != "main"},
                             k_top, base)
    rec["entry_top"] = top(d("entry"), k_top, base)
    rec["mechanisms"] = {k: round(v) for k, v in sorted(d("mech").items(), key=lambda kv: -kv[1])
                         if abs(v) >= 0.5}
    chain_raw = d("chain")
    site_raw = d("site")
    # resolve the JS sites that matter to file:line, then regroup chains
    # by (bucket, runtime entry, JS function, JS line) -- the leaf is kept
    # as the dominant leaf of that group.
    keep = sorted(chain_raw.items(), key=lambda kv: -kv[1])[:4000]
    addrs = sorted({int(k.split("\t")[3].split("@")[1]) for k, _v in keep if k.split("\t")[3].split("@")[1]}
                   | {int(k.split("@")[1]) for k, v in sorted(site_raw.items(), key=lambda kv: -kv[1])[:500]
                      if k.split("@")[1]})
    locs = addr2line(binary, addrs)

    def loc_of(a: str):
        full = locs.get(int(a)) if a else None
        if not full:
            return None
        m = re.match(r"^(.*):(\d+)(?::\d+)?$", full)
        snip = ""
        if m and not m.group(1).startswith("?"):
            ln = int(m.group(2))
            try:
                lines = srcidx.lines_of(m.group(1))
                if 0 < ln <= len(lines) and len(lines[ln - 1]) <= 500:
                    snip = " `" + lines[ln - 1].strip()[:56].replace("|", "/").replace("`", "'") + "`"
            except OSError:
                pass
        return short_loc(full) + snip

    def where(fn: str, loc: str | None) -> str | None:
        # DWARF line (with --debug-symbols) wins; the source index adds
        # the definition site and, for minified files, the original line.
        hint = None
        mh = re.search(r":(\d+)(?: `|$)", loc or "")
        if mh:
            hint = int(mh.group(1))
        found = srcidx.locate(fn, hint) if fn.startswith("perry_") else None
        if loc and found and "(orig" not in found:
            return loc
        return found or loc

    groups: dict = {}
    for k, v in keep:
        b, leaf, entry, site = k.split("\t")
        fn, a = site.split("@")
        g = groups.setdefault((b, entry, fn, loc_of(a)), {"per_iter": 0.0, "leaves": Counter()})
        g["per_iter"] += v
        g["leaves"][leaf] += v
    rec["chains"] = []
    for (b, entry, fn, loc), g in sorted(groups.items(), key=lambda kv: -kv[1]["per_iter"])[:args.top_chains]:
        leaf, lv = g["leaves"].most_common(1)[0]
        rec["chains"].append({"bucket": b, "entry": entry, "js_fn": js_name(fn), "js_loc": where(fn, loc),
                              "leaf": leaf, "leaf_share": round(lv / g["per_iter"], 2) if g["per_iter"] else None,
                              "per_iter": round(g["per_iter"]), "pct": round(100 * g["per_iter"] / base, 2)})
    fn_groups: Counter = Counter()
    for k, v in site_raw.items():
        fn, a = k.split("@")
        fn_groups[(fn, loc_of(a) if int(a or 0) in locs else None)] += v
    rec["js_sites"] = [{"js_fn": js_name(fn), "js_loc": where(fn, loc), "per_iter": round(v),
                        "pct": round(100 * v / base, 2)}
                       for (fn, loc), v in fn_groups.most_common(k_top)]


# ---------------------------------------------------------------- driver

def run(args, pb) -> None:
    """`pb` is the package_bench module (servers, manifest, lock helpers)."""
    manifest = pb.load_manifest()
    wls = [w for w in pb.select_workloads(manifest, args.filter)
           if not w["id"].startswith("control/") or args.include_control]
    out_json = Path(args.out)
    doc = json.loads(out_json.read_text()) if out_json.exists() else {"workloads": {}}
    if getattr(args, "reanalyze", False):
        # Re-derive every table from the saved stacks (after editing the
        # bucket rules): no perf, no binaries run, no lock needed.
        srcidx = SourceIndex(pb.PKG_DIR)
        stack_dir = out_json.parent / (out_json.stem + "-stacks")
        for w in wls:
            rec = doc["workloads"].get(w["id"])
            sp = stack_dir / (pb.bin_name(w["id"]) + ".json.gz")
            if not rec or rec.get("status") != "OK" or not sp.exists():
                continue
            a1, a2 = load_stacks(sp)
            analyze(rec, a1, a2, Path(rec["binary"]), srcidx, args)
            pb.log(f"re-analyzed {w['id']}")
        out_json.write_text(json.dumps(doc, indent=1, sort_keys=True))
        write_markdown(doc, out_json.with_suffix(".md"))
        return
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
    srcidx = SourceIndex(pb.PKG_DIR)
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
        binary = (Path(args.perry_bin_dir) / pb.bin_name(wid)).resolve()
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
                rc, o, e, ins, _w = perf_stat([args.node, "--no-warnings", src, str(n), str(warm)], env,
                                              pb.PKG_DIR, args.timeout)
                i_node[n], node_out[n] = ins, (o if rc == 0 else None)
            if node_pi is None and None not in i_node.values():
                node_pi = (i_node[n2] - i_node[n1]) / (n2 - n1)
        rec["node_per_iter"] = node_pi
        # Perry: correctness + instructions
        i_p, wall = {}, {}
        for n in (n1, n2):
            rc, o, e, ins, wall[n] = perf_stat([str(binary), str(n), str(warm)], env, pb.PKG_DIR, args.timeout)
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
        freq = int(min(args.max_freq, max(50, args.target_samples / max(0.05, wall[n2] or 1.0))))
        rec["sample_freq"] = freq
        aggs = {}
        gen_addr = nm_addrs(binary)
        if not gen_addr:
            rec.update(status="FAIL", reason="no generated-code symbols: compile with PERRY_KEEP_SYMBOLS=1")
            save()
            continue
        for n in (n1, n2):
            data = tmp / f"pkgattr-{os.getpid()}-{n}.data"
            rc, _ = perf_record([str(binary), str(n), str(warm)], env, pb.PKG_DIR, args.timeout, freq,
                                args.dwarf_stack, data, args.callgraph_mode)
            aggs[n] = aggregate(data, binary, gen_addr)
            data.unlink(missing_ok=True)
        stack_dir = out_json.parent / (out_json.stem + "-stacks")
        stack_dir.mkdir(parents=True, exist_ok=True)
        save_stacks(stack_dir / (pb.bin_name(wid) + ".json.gz"), aggs[n1], aggs[n2])
        rec["binary"] = str(binary)
        analyze(rec, aggs[n1], aggs[n2], binary, srcidx, args)
        rec["status"] = "OK"
        pb.log(f"{wid}: perry {perry_pi:,.0f}/iter node {node_pi or 0:,.0f}/iter; unwind->main "
               f"{rec['unwind_reached_main']:.0%}; top buckets "
               + ", ".join(f"{k} {100 * v / (rec['perry_per_iter'] or 1):.0f}%"
                           for k, v in list(rec["buckets"].items())[:4]))
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


def control_floor(doc: dict) -> float:
    """Per-iteration harness floor = max(0, control perry - control node)."""
    c = doc["workloads"].get("control/bare_loop") or {}
    if c.get("status") != "OK" or c.get("node_per_iter") is None:
        return 0.0
    return max(0.0, c["perry_per_iter"] - c["node_per_iter"])


def rank_buckets(doc: dict, min_ratio: float = 2.0, floor: float = 0.0):
    wl = {}
    for w, r in doc["workloads"].items():
        if r.get("status") != "OK" or w.startswith("control/") or (r.get("ratio") or 0) < min_ratio:
            continue
        if floor:
            r = dict(r, excess_per_iter=r["excess_per_iter"] - floor)
        wl[w] = r
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


def short_rt(sym: str) -> str:
    s = re.sub(r"<[^<>]*>", "", sym).replace("perry_runtime::", "").replace("perry_stdlib::", "")
    return (s if len(s) <= 70 else "…" + s[-69:]).replace("|", "/")


def short_fn(sym: str) -> str:
    s = re.sub(r"^perry_(fn|closure|method)_", "", sym).replace("node_modules_", "")
    return (s if len(s) <= 70 else s[:69] + "…").replace("|", "/")


def write_markdown(doc: dict, path: Path) -> None:
    floor = control_floor(doc)
    ranked, pkg_share, wl = rank_buckets(doc)
    ranked_f = {r["bucket"]: r for r in rank_buckets(doc, floor=floor)[0]} if floor else {}
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
         f"Control floor (control perry − node, clamped at 0): **{floor:.0f} instr/iter**"
         + (" — the last column re-ranks with it subtracted from every workload's excess." if floor else
            " — nothing to subtract, so the floor-adjusted ranking is identical."), "",
         "| # | bucket | equal-weight % of excess | abs-weight % | packages where ≥5% | equal-weight, floor-adjusted |",
         "|---|---|---:|---:|---|---:|"]
    for i, r in enumerate(ranked, 1):
        fa = ranked_f.get(r["bucket"], r)["equal_weight_pct"]
        L.append(f"| {i} | {BUCKET_LABELS.get(r['bucket'], r['bucket'])} (`{r['bucket']}`) | "
                 f"{r['equal_weight_pct']:.1f} | {r['abs_weight_pct']:.1f} | {len(r['packages_ge5pct'])}: "
                 f"{', '.join(r['packages_ge5pct'])} | {fa:.1f} |")
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
              "Top chains: JS site → runtime entry the generated code called → bucket (dominant leaf):", "",
              "| instr/iter | % | JS function | JS source | runtime entry | bucket | dominant leaf |",
              "|---:|---:|---|---|---|---|---|"]
        for c in r["chains"][:12]:
            L.append(f"| {fmt_n(c['per_iter'])} | {c['pct']} | `{short_fn(c['js_fn'])}` | {c['js_loc'] or '—'} | "
                     f"`{short_rt(c['entry'])}` | {c['bucket']} | `{short_rt(c['leaf'])}` "
                     f"({int(100 * (c.get('leaf_share') or 0))}%) |")
        L += ["", "Top inclusive runtime functions: " + "; ".join(
            f"`{short_rt(n)}` {p}%" for n, _v, p in r["incl_top"][:8])]
        L += ["", "Top self: " + "; ".join(f"`{short_rt(n)}` {p}%" for n, _v, p in r["self_top"][:8])]
        L += ["", "Mechanism view (leaf-first): " + ", ".join(
            f"{k} {100 * v / (r['perry_per_iter'] or 1):.0f}%" for k, v in list(r.get("mechanisms", {}).items())[:6])]
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
