#!/usr/bin/env python3
"""The runtime's default hot-text order: record, build, check.

The link places the runtime functions listed in
crates/perry/src/commands/compile/link/hot_text_order.txt at the front of
`.text` (see hot_text_order.rs beside it). This script makes that list and
keeps it honest. Linux only: recording uses valgrind's callgrind.

  record -o OUT.fns BIN [ARGS...]
      Run BIN (built with PERRY_KEEP_SYMBOLS=1; on x86-64 also
      PERRY_TARGET_CPU=x86-64-v3, callgrind has no AVX-512) and write the text
      symbols that executed at least once: `symbol size address`.

  build --archive libperry_runtime.a [--archive ...] -o ORDER.txt
        [--report FILE] [--held DIR] [--min-programs N] [--show REGEX] DIR_OR_FNS...
      Build the list from recordings, one program per .fns file. Functions are
      keyed hash-free, counted per program, and ordered by how many programs
      execute them: the common core first, then decreasing frequency, each
      class clustered by which programs use it. Functions fewer than
      --min-programs (default 2) recordings executed are not listed and stay
      behind the hot run. The report holds the class table and a model of the
      64 KiB text windows each program maps with and without the order.

  check --perry PERRY [--runtime-dir DIR] [--program FILE.ts[::ARGS]]...
        [--max-core-missing N] [--max-missing-pct P] [--max-outside-pct P]
      The gate. (1) Every listed name must still exist in the runtime
      archives: a renamed function of the core tier fails at once, the long
      tail may lose --max-missing-pct percent. (2) Order coverage: compile the
      smoke program(s), run them under callgrind, and fail when more than
      --max-outside-pct percent of the executed runtime bytes lie outside the
      hot run. Exit status 1 = stale list: fix the renamed lines or rebuild
      the list.

  selftest
      The key function against the vectors hot_text_order.rs tests.
"""
import argparse
import collections
import os
import re
import subprocess
import sys
import tempfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ORDER = os.path.join(ROOT, "crates/perry/src/commands/compile/link/hot_text_order.txt")
HEX = "0123456789abcdefABCDEF"

# A class, an array, a loop, string building, a Map, JSON and a regex: the
# runtime paths nearly every program takes.
SMOKE = r"""
class P { x: number; constructor(x: number) { this.x = x; } norm(): number { return Math.abs(this.x) + 1; } }
const a: P[] = [];
for (let i = 0; i < 100; i++) a.push(new P(i - 50));
let s = 0;
for (const p of a) s += p.norm();
const m = new Map<string, number>();
for (let i = 0; i < 50; i++) m.set("k" + (i % 7), i);
const text = JSON.stringify({ s, keys: [...m.keys()], words: "a b  c".split(/\s+/) });
console.log("smoke " + text + " " + process.argv.length);
"""


def symbol_key(symbol):
    """The hash-free identity of a symbol; the twin of hot_text_order.rs `symbol_key`."""
    at = symbol.find(".llvm.")
    if at >= 0:
        symbol = symbol[:at]
    if not symbol.startswith("_R") or not symbol.isascii():
        return symbol
    b = symbol
    n = len(b)
    out = ["_R"]
    i = 2
    while i < n:
        c = b[i]
        if c in ".$":
            out.append(b[i:])
            break
        if c.isdigit():
            j = i
            length = 0
            while j < n and b[j].isdigit():
                length = length * 10 + int(b[j])
                j += 1
            if j < n and b[j] == "_":
                j += 1
            end = min(j + length, n)
            out.append(b[i:end])
            i = end
        elif c in "Bs":
            j = i + 1
            while j < n and b[j].isalnum():
                j += 1
            out.append(c)
            if j < n and b[j] == "_":
                out.append("_")
                i = j + 1
            else:
                i += 1
        elif c == "K" and i + 1 < n and b[i + 1].islower() and b[i + 1] != "p":
            j = i + 2
            while j < n and (b[j] in HEX or b[j] == "n"):
                j += 1
            if j < n and b[j] == "_":
                out.append(b[i : j + 1])
                i = j + 1
            else:
                out.append("K")
                i += 1
        elif c in "LG":
            j = i + 1
            while j < n and j - i <= 6 and b[j].isalnum():
                j += 1
            if j < n and b[j] == "_":
                out.append(b[i : j + 1])
                i = j + 1
            else:
                out.append(c)
                i += 1
        else:
            out.append(c)
            i += 1
    return "".join(out)


KEY_VECTORS = [
    ("js_string_concat", "js_string_concat"),
    ("mi_process_attach", "mi_process_attach"),
    ("_RNvNtNtCsXbwMRDrjNy_13perry_runtime2gc6policy25gc_budgeted_start_or_step",
     "_RNvNtNtCs_13perry_runtime2gc6policy25gc_budgeted_start_or_step"),
    ("_RNvNtNtCsklhtP6PwTF_13perry_runtime2gc6policy25gc_budgeted_start_or_step",
     "_RNvNtNtCs_13perry_runtime2gc6policy25gc_budgeted_start_or_step"),
    ("_RINvMs_NtNtNtCsXbwMRDrjNy_13perry_runtime2gc5roots15runtime_handlesNtB5_18RuntimeHandleScope16root_raw_mut_ptrNtNtBb_6object12ObjectHeaderEBb_",
     "_RINvMs_NtNtNtCs_13perry_runtime2gc5roots15runtime_handlesNtB_18RuntimeHandleScope16root_raw_mut_ptrNtNtB_6object12ObjectHeaderEB_"),
    ("_RNCINvNtNtCsXbwMRDrjNy_13perry_runtime5arena12owned_stores22walk_from_space_ownersNCNvB4_50finalize_dead_copied_minor_from_space_owned_storess0_0E0B8_",
     "_RNCINvNtNtCs_13perry_runtime5arena12owned_stores22walk_from_space_ownersNCNvB_50finalize_dead_copied_minor_from_space_owned_storess_0E0B_"),
    ("_RNvNtNtCsXbwMRDrjNy_13perry_runtime5array15header_gc_slots25store_array_slot_resolved.llvm.2236164344119935801",
     "_RNvNtNtCs_13perry_runtime5array15header_gc_slots25store_array_slot_resolved"),
    ("_RINvNtNtNtCsXbwMRDrjNy_13perry_runtime6object6shapes12layout_probe9owned_getKb1_Kb0_EB8_",
     "_RINvNtNtNtCs_13perry_runtime6object6shapes12layout_probe9owned_getKb1_Kb0_EB_"),
    ("_RNvMs4_NtNtCsklhtP6PwTF_5alloc11collections9vec_dequeINtB5_8VecDequeNtNtCsXbwMRDrjNy_13perry_runtime3map16CompactionRecordE4growB18_",
     "_RNvMs_NtNtCs_5alloc11collections9vec_dequeINtB_8VecDequeNtNtCs_13perry_runtime3map16CompactionRecordE4growB_"),
]


def selftest():
    bad = 0
    for symbol, key in KEY_VECTORS:
        for got, want, what in ((symbol_key(symbol), key, symbol), (symbol_key(key), key, key)):
            if got != want:
                bad += 1
                print(f"FAIL key({what})\n  got  {got}\n  want {want}")
    print("selftest", "FAILED" if bad else "ok", f"({len(KEY_VECTORS)} vectors)")
    return 1 if bad else 0


def text_symbols(binary):
    """[(name, size, address)] of the defined text symbols with a size."""
    nm = subprocess.run(["nm", "-S", "--defined-only", binary], check=True, capture_output=True, text=True).stdout
    out = []
    for line in nm.splitlines():
        p = line.split()
        if len(p) == 4 and p[2] in "tTwW":
            out.append((p[3], int(p[1], 16), int(p[0], 16)))
    return out


def all_symbols(binary):
    nm = subprocess.run(["nm", "--defined-only", binary], check=True, capture_output=True, text=True).stdout
    return {p[-1]: int(p[0], 16) for p in (l.split() for l in nm.splitlines()) if len(p) >= 3}


def js_text_range(binary):
    """The address range of the compiled program's own code, which the order never places."""
    out = subprocess.run(["readelf", "-SW", binary], check=True, capture_output=True, text=True).stdout
    for line in out.splitlines():
        m = re.search(r"\]\s+perry_js_text\s+\S+\s+([0-9a-f]+)\s+[0-9a-f]+\s+([0-9a-f]+)", line)
        if m:
            start = int(m.group(1), 16)
            return start, start + int(m.group(2), 16)
    return 0, 0


def executed_symbols(binary, args, cwd=None, timeout=3600):
    """Run under callgrind; the names of the functions that ran at least once."""
    with tempfile.TemporaryDirectory(prefix="perry-hot-text-") as tmp:
        out = os.path.join(tmp, "callgrind.out")
        subprocess.run(
            ["valgrind", "--tool=callgrind", "--demangle=no", "--dump-instr=no", "-q",
             f"--callgrind-out-file={out}", binary, *args],
            cwd=cwd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, stdin=subprocess.DEVNULL, timeout=timeout)
        ran = set()
        if os.path.exists(out):
            for line in open(out, errors="replace"):
                m = re.match(r"^c?fn=\(\d+\)(?: (.+))?$", line.rstrip("\n"))
                if m and m.group(1):
                    ran.add(m.group(1))
        return ran


def record(binary, args, cwd=None):
    """[(name, size, address)] of the binary's text symbols that executed."""
    ran = executed_symbols(binary, args, cwd)
    return [s for s in text_symbols(binary) if s[0] in ran or s[0].split("'")[0] in ran]


def cmd_record(a):
    rows = record(os.path.abspath(a.binary), a.args)
    with open(a.output, "w") as o:
        for name, size, address in rows:
            o.write(f"{name} {size} {address:x}\n")
    print(f"{len(rows)} executed functions, {sum(r[1] for r in rows)} B -> {a.output}")
    return 0 if rows else 1


def archive_keys(archives):
    """key -> total size of the function symbols the archives define."""
    sizes = collections.Counter()
    for archive in archives:
        nm = subprocess.run(["nm", "-S", "--defined-only", archive], capture_output=True, text=True).stdout
        for line in nm.splitlines():
            p = line.split()
            if len(p) == 4 and p[2] in "tTwW":
                sizes[symbol_key(p[3])] += int(p[1], 16)
    return sizes


def load_recordings(paths):
    """(program -> {key: bytes}, program -> {key: address in the recorded binary})."""
    files = []
    for path in paths:
        if os.path.isdir(path):
            files += sorted(os.path.join(path, f) for f in os.listdir(path) if f.endswith(".fns"))
        else:
            files.append(path)
    progs = {}
    where = {}
    for f in files:
        keys = {}
        addr = {}
        for line in open(f):
            p = line.split()
            if len(p) >= 2:
                k = symbol_key(p[0])
                keys[k] = keys.get(k, 0) + int(p[1])
                if len(p) >= 3:
                    addr.setdefault(k, int(p[2], 16))
        if keys:
            progs[os.path.basename(f)[:-4]] = keys
            where[os.path.basename(f)[:-4]] = addr
    return progs, where


TIERS = (("core", 0.90, "executed by at least 90% of the programs"),
         ("common", 0.50, "50% to 90%"),
         ("frequent", 0.10, "10% to 50%"),
         ("occasional", 0.0, "fewer than 10%"))


def cluster(keys, bits):
    """Chain the keys so that functions the same programs execute end up adjacent."""
    if len(keys) < 3:
        return list(keys)
    # Equal program sets first: they always go together.
    groups = collections.defaultdict(list)
    for k in keys:
        groups[bits[k]].append(k)
    sigs = sorted(groups, key=lambda s: (-bin(s).count("1"), s))
    order = [sigs.pop(0)]
    while sigs:
        last = order[-1]
        best = max(range(len(sigs)), key=lambda i: (bin(sigs[i] & last).count("1") / bin(sigs[i] | last).count("1"), -i))
        order.append(sigs.pop(best))
    return [k for s in order for k in sorted(groups[s])]


def windows(layout_pos, sizes, keys, window=65536):
    """How many `window`-byte windows of the hot run the given keys touch."""
    seen = set()
    for k in keys:
        if k in layout_pos:
            a = layout_pos[k]
            seen.update(range(a // window, (a + max(sizes[k], 1) - 1) // window + 1))
    return len(seen)


def cmd_build(a):
    progs, where = load_recordings(a.recordings)
    held, held_where = load_recordings([a.held]) if a.held else ({}, {})
    where.update(held_where)
    for name in held:
        progs.pop(name, None)
    if not progs:
        sys.exit("no recordings")
    in_archive = archive_keys(a.archive)
    # A key's size is what a link really carries of it (an archive pair holds
    # some functions twice, and one key can name several instantiations).
    arch = collections.Counter()
    for keys in list(progs.values()) + list(held.values()):
        for k, size in keys.items():
            if k in in_archive:
                arch[k] = max(arch[k], size)
    n = len(progs)
    names = sorted(progs)
    count = collections.Counter()
    bits = collections.Counter()
    for i, name in enumerate(names):
        for k in progs[name]:
            if k in arch:
                count[k] += 1
                bits[k] |= 1 << i
    tiers = []
    rest = {k for k in count if count[k] >= a.min_programs}
    for label, floor, text in TIERS:
        members = [k for k in rest if count[k] >= floor * n]
        rest -= set(members)
        by_count = sorted(members, key=lambda k: (-count[k], k))
        # The core runs everywhere: keep it in count order. The other tiers
        # are clustered by which programs execute them.
        ordered = by_count if label == "core" else cluster(by_count, bits)
        tiers.append((label, text, ordered))
    listed = [k for _, _, ks in tiers for k in ks]
    pos = {}
    at = 0
    for k in listed:
        pos[k] = at
        at += (arch[k] + 15) & ~15
    never = {k: s for k, s in in_archive.items() if k not in count}
    report = [f"programs {n}; archive functions {len(in_archive)} keys, {sum(in_archive.values())} B (every archive member, both archives)",
              "| class | functions | bytes |", "|---|--:|--:|"]
    for label, text, ks in tiers:
        report.append(f"| {label}: {text} | {len(ks)} | {sum(arch[k] for k in ks)} |")
    report.append(f"| never executed by the corpus (archive bytes, before the link drops unreferenced code) | {len(never)} | {sum(never.values())} |")
    report.append(f"hot run {at} B = {(at + 65535) // 65536} windows of 64 KiB")

    def spread(group, label):
        # A model of the text windows a program maps: the windows of the hot
        # run it touches, plus one window per 64 KiB of recorded address its
        # unlisted functions occupy (they stay in archive order behind the run).
        if not group:
            return
        hot, cold, before = [], [], []
        for name, ks in group.items():
            hot.append(windows(pos, arch, ks))
            addr = where.get(name, {})
            cold.append(len({addr[k] >> 16 for k in ks if k in arch and k not in pos and k in addr}))
            before.append(len({addr[k] >> 16 for k in ks if k in arch and k in addr}))
        if a.show:
            for (name, ks), h, c, b in zip(group.items(), hot, cold, before):
                if re.search(a.show, name):
                    exe_bytes = sum(s for k, s in ks.items() if k in arch)
                    report.append(f"  {name}: executed {exe_bytes} B = {(exe_bytes + 65535) // 65536} windows if packed; unordered {b}, ordered {h} + {c}")
        total = sorted(h + c for h, c in zip(hot, cold))
        out = sorted(sum(s for k, s in ks.items() if k in arch and k not in pos) for ks in group.values())
        exe = sorted(sum(s for k, s in ks.items() if k in arch) for ks in group.values())
        mid = len(group) // 2
        report.append(f"{label}: {len(group)} programs; executed runtime bytes median {exe[mid]}; outside the hot run median/max {out[mid]}/{out[-1]} B; "
                      f"modelled 64 KiB text windows min/median/mean/max: unordered {min(before)}/{sorted(before)[mid]}/{sum(before) / len(before):.1f}/{max(before)}, "
                      f"ordered {total[0]}/{total[mid]}/{sum(total) / len(total):.1f}/{total[-1]} (hot run {sorted(hot)[mid]} + behind it {sorted(cold)[mid]})")
    spread(progs, "train")
    spread(held, "held out")
    with open(a.output, "w") as o:
        o.write("# The runtime's default hot-text order (hot_text_order.rs). Generated by\n"
                "# scripts/hot_text_order.py build; one hash-free function key per line,\n"
                "# placed in this order at the front of .text. `# tier` lines start a class.\n"
                f"# Recorded over {n} programs.\n")
        for label, text, ks in tiers:
            o.write(f"# tier {label}: {text}; {len(ks)} functions, {sum(arch[k] for k in ks)} bytes\n")
            for k in ks:
                o.write(k + "\n")
    text = "\n".join(report)
    print(text)
    if a.report:
        open(a.report, "w").write(text + "\n")
    return 0


def read_order(path):
    """(keys in order, number of keys of the first tier)."""
    keys = []
    core = None
    tiers = 0
    for line in open(path):
        line = line.strip()
        if line.startswith("# tier "):
            tiers += 1
            if tiers == 2:
                core = len(keys)
        elif line and not line.startswith("#"):
            keys.append(line)
    return keys, len(keys) if core is None else core


def cmd_check(a):
    keys, core = read_order(a.order)
    rt = a.runtime_dir or os.path.dirname(os.path.abspath(a.perry))
    archives = [p for p in (os.path.join(rt, n) for n in ("libperry_runtime.a", "libperry_stdlib.a")) if os.path.exists(p)]
    if not archives:
        sys.exit(f"no runtime archive in {rt}")
    failed = False
    arch = archive_keys(archives)
    missing = [(i, k) for i, k in enumerate(keys) if k not in arch]
    core_missing = [k for i, k in missing if i < core]
    pct = 100.0 * len(missing) / max(len(keys), 1)
    print(f"names: {len(keys) - len(missing)} of {len(keys)} listed functions exist in {', '.join(os.path.basename(p) for p in archives)}; "
          f"missing {len(missing)} ({pct:.2f}%), of the core tier {len(core_missing)} of {core}")
    for k in core_missing[:40]:
        print(f"  missing core function: {k}")
    for i, k in missing[:10 if not a.verbose else None]:
        if i >= core:
            print(f"  missing: {k}")
    if len(core_missing) > a.max_core_missing:
        failed = True
        print(f"FAIL: {len(core_missing)} core-tier functions no longer exist (allowed {a.max_core_missing}). "
              f"A renamed function keeps its place if its line in {os.path.relpath(a.order, ROOT)} is renamed too; otherwise rebuild the list.")
    if pct > a.max_missing_pct:
        failed = True
        print(f"FAIL: {pct:.2f}% of the listed functions no longer exist (allowed {a.max_missing_pct}%): rebuild the list.")

    programs = [(p.split("::")[0], p.split("::")[1].split() if "::" in p else []) for p in a.program]
    with tempfile.TemporaryDirectory(prefix="perry-hot-text-") as tmp:
        if not programs:
            with open(os.path.join(tmp, "smoke.ts"), "w") as o:
                o.write(SMOKE)
            programs = [(os.path.join(tmp, "smoke.ts"), [])]
        have_valgrind = subprocess.run(["sh", "-c", "command -v valgrind"], capture_output=True).returncode == 0
        if not have_valgrind and not a.require_coverage:
            print("coverage: skipped (no valgrind)")
            programs = []
        for index, (source, args) in enumerate(programs):
            binary = os.path.join(tmp, f"prog{index}")
            env = dict(os.environ, PERRY_KEEP_SYMBOLS="1", PERRY_NO_CACHE="1", PERRY_RUNTIME_DIR=rt)
            env.pop("PERRY_HOT_TEXT_ORDER", None)
            if os.uname().machine == "x86_64":
                env.setdefault("PERRY_TARGET_CPU", "x86-64-v3")
            c = subprocess.run([a.perry, "compile", os.path.basename(source), "-o", binary, *a.compile_arg],
                               cwd=os.path.dirname(os.path.abspath(source)), env=env, capture_output=True, text=True)
            if c.returncode != 0:
                print(f"FAIL: compiling {source}:\n{c.stdout[-2000:]}{c.stderr[-2000:]}")
                failed = True
                continue
            syms = all_symbols(binary)
            lo, hi = syms.get("__perry_rt_hot_text_start"), syms.get("__perry_rt_hot_text_end")
            if lo is None or hi is None:
                print(f"FAIL: {source}: the link placed no hot run (no __perry_rt_hot_text_start)")
                failed = True
                continue
            js_lo, js_hi = js_text_range(binary)
            inside = outside = 0
            strays = []
            for name, size, address in record(binary, args, os.path.dirname(os.path.abspath(source))):
                if js_lo <= address < js_hi:
                    continue
                if lo <= address < hi:
                    inside += size
                else:
                    outside += size
                    strays.append((size, name))
            total = inside + outside
            out_pct = 100.0 * outside / max(total, 1)
            print(f"coverage: {os.path.basename(source)} {' '.join(args)}: executed runtime text {total} B, hot run {hi - lo} B; "
                  f"outside the hot run {outside} B = {out_pct:.2f}% ({len(strays)} functions)")
            for size, name in sorted(strays, reverse=True)[:8 if not a.verbose else None]:
                print(f"  outside: {size:6d} {name}")
            if total == 0 or out_pct > a.max_outside_pct:
                failed = True
                print(f"FAIL: more than {a.max_outside_pct}% of the executed runtime bytes are outside the hot run: rebuild the list.")
    print("hot-text order:", "STALE" if failed else "ok")
    return 1 if failed else 0


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    r = sub.add_parser("record")
    r.add_argument("-o", "--output", required=True)
    r.add_argument("binary")
    r.add_argument("args", nargs=argparse.REMAINDER)
    b = sub.add_parser("build")
    b.add_argument("--archive", action="append", required=True)
    b.add_argument("-o", "--output", default=ORDER)
    b.add_argument("--report")
    b.add_argument("--min-programs", type=int, default=2, help="list only functions at least this many programs execute")
    b.add_argument("--show", help="also report the model for the programs matching this regex")
    b.add_argument("--held", help="a directory of recordings to keep out of the list (reported as held out)")
    b.add_argument("recordings", nargs="+")
    c = sub.add_parser("check")
    c.add_argument("--perry", required=True)
    c.add_argument("--runtime-dir")
    c.add_argument("--order", default=ORDER)
    c.add_argument("--program", action="append", default=[], help="FILE.ts or FILE.ts::ARGS; default: a built-in smoke program")
    c.add_argument("--compile-arg", action="append", default=[])
    c.add_argument("--max-core-missing", type=int, default=0)
    c.add_argument("--max-missing-pct", type=float, default=2.0)
    c.add_argument("--max-outside-pct", type=float, default=5.0)
    c.add_argument("--require-coverage", action="store_true")
    c.add_argument("-v", "--verbose", action="store_true")
    sub.add_parser("selftest")
    a = ap.parse_args()
    if a.cmd == "selftest":
        return selftest()
    return {"record": cmd_record, "build": cmd_build, "check": cmd_check}[a.cmd](a)


if __name__ == "__main__":
    sys.exit(main())
