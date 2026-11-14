#!/usr/bin/env python3
"""The runtime's default hot-data order: record, build, check.

The link places the runtime's read-only data listed in
crates/perry/src/commands/compile/link/hot_data_order.txt at the front of
`.rodata` and `.data.rel.ro` (hot_data_order.rs beside it), as the hot-text
order does for `.text`. This script makes that list and keeps it honest.
Linux only: recording uses valgrind's lackey (every load and store).

A line of the list is the hash-free key of one input section of the runtime
archives (`section_key`): `r:` for `.rodata.*`, `d:` for `.data.rel.ro.*`,
then the hot-text key of the symbol the section is named after, or, for an
unnamed constant (`.rodata..Lanon.<hash>.<n>`), `#` and a digest of its bytes
and relocations. Merged sections (`.rodata.str1.1`, `.rodata.cst16`, ...)
are pooled by the linker and never listed.

  record --map MAP -o OUT.keys [--archive A]... BIN [ARGS...]
      Run BIN under lackey (built with PERRY_KEEP_SYMBOLS=1, on x86-64 also
      PERRY_TARGET_CPU=x86-64-v3, and PERRY_LINK_MAP=MAP) and write the keys
      of the runtime archive sections it read: `key size touched-bytes`.

  build --archive A [--archive ...] -o ORDER.txt [--report FILE]
        [--held DIR] [--min-programs N] DIR_OR_KEYS...
      Build the list from recordings, one program per .keys file: the core
      every program reads first, then decreasing frequency, each class
      clustered by which programs read it (the hot-text builder's tiers).

  check --perry PERRY [--runtime-dir DIR] [--program FILE.ts[::ARGS]]...
  check --names-only --runtime-dir DIR
      The gate. (1) Names: every listed key must still exist in the runtime
      archives; a missing core key fails at once, the rest may lose
      --max-missing-pct percent. (2) Coverage: compile the smoke program(s),
      run them under lackey and fail when more than --max-outside-pct percent
      of the runtime data sections they read lie outside the hot runs.

  selftest
      The key function against the vectors hot_data_order.rs tests.
"""
import argparse
import collections
import os
import re
import struct
import subprocess
import sys
import tempfile

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import hot_text_order as hto  # noqa: E402  (symbol_key, cluster, TIERS, SMOKE)

ROOT = hto.ROOT
ORDER = os.path.join(ROOT, "crates/perry/src/commands/compile/link/hot_data_order.txt")

FNV_OFFSET = 0xCBF29CE484222325
FNV_PRIME = 0x100000001B3
M64 = (1 << 64) - 1


def fnv(data, h=FNV_OFFSET):
    for b in data:
        h = ((h ^ b) * FNV_PRIME) & M64
    return h


# Name prefixes of orderable data sections, longest first; the rest of the
# name is the symbol (or `.L...` for an unnamed constant).
PREFIXES = ((".rodata.unlikely.", "r"), (".rodata.hot.", "r"), (".rodata.", "r"),
            (".data.rel.ro.local.", "d"), (".data.rel.ro.", "d"))
LITERAL = re.compile(r"^[A-Za-z0-9_.$]+$")
SHF_ALLOC, SHF_MERGE, SHT_NOBITS, SHT_RELA, SHT_SYMTAB, SHT_SYMTAB_SHNDX = 0x2, 0x10, 8, 4, 2, 18
STT_SECTION = 3


def target_section_name(name):
    """The hash-free name of a relocation's target section: a function or
    data section by its symbol's key, an unnamed or pooled one by its kind
    (the part before `..L`)."""
    for prefix in (".text.unlikely.", ".text.hot.", ".text."):
        if name.startswith(prefix) and len(name) > len(prefix) and not name[len(prefix):].startswith(".L"):
            return prefix + hto.symbol_key(name[len(prefix):])
    split = split_name(name)
    if split and not split[1].startswith(".L"):
        return f"{split[0]}:{hto.symbol_key(split[1])}"
    at = name.find("..L")
    return name[:at] if at >= 0 else name


def split_name(name):
    """(kind letter, rest) of an orderable data section name, or None."""
    if not LITERAL.match(name):
        return None
    for prefix, kind in PREFIXES:
        if name.startswith(prefix) and len(name) > len(prefix):
            return kind, name[len(prefix):]
    return None


def section_key(name, data, size, relocs):
    """The twin of hot_data_order.rs `section_key`. `relocs` is a list of
    (offset, r_type, addend, target) with target ("S", key) for a named
    symbol or ("#", name) for a section symbol, `name` being the target
    section's hash-free name (`target_section_name`); a section-symbol
    relocation's addend is left out (it is an offset into a pool that moves
    with every edit of the pool)."""
    split = split_name(name)
    if split is None:
        return None
    kind, rest = split
    if not rest.startswith(".L"):
        return f"{kind}:{hto.symbol_key(rest)}"
    stream = bytearray(struct.pack("<Q", size))
    stream += data
    for offset, r_type, addend, target in sorted(relocs, key=lambda r: r[0]):
        if target[0] == "S":
            stream += struct.pack("<QIq", offset, r_type, addend)
            stream += b"S" + target[1].encode() + b"\0"
        else:
            stream += struct.pack("<QI", offset, r_type)
            stream += b"#" + target[1].encode() + b"\0"
    return f"{kind}:#{fnv(stream):016x}"


def ar_members(path):
    """[(member name, bytes)] of a System V / GNU archive."""
    data = open(path, "rb").read()
    if not data.startswith(b"!<arch>\n"):
        return []
    out, names, at = [], b"", 8
    while at + 60 <= len(data):
        head = data[at:at + 60]
        name = head[:16].decode("latin-1").rstrip()
        size = int(head[48:58].decode().strip())
        body = data[at + 60:at + 60 + size]
        at += 60 + size + (size & 1)
        if name == "//":
            names = body
            continue
        if name in ("/", "/SYM64/"):
            continue
        if name.startswith("/") and name[1:].isdigit():
            off = int(name[1:])
            name = names[off:names.index(b"\n", off)].decode("latin-1")
        out.append((name.rstrip("/"), body))
    return out


def elf_data_keys(obj):
    """{section name: (key, size, flags)} of the orderable data sections of an ELF64 LE relocatable object."""
    if obj[:4] != b"\x7fELF" or obj[4] != 2 or obj[5] != 1:
        return {}
    shoff, = struct.unpack_from("<Q", obj, 0x28)
    shnum, shstrndx = struct.unpack_from("<HH", obj, 0x3C)
    if shoff == 0:
        return {}
    if shnum == 0:
        shnum, = struct.unpack_from("<Q", obj, shoff + 0x20)
    if shstrndx == 0xFFFF:
        shstrndx, = struct.unpack_from("<I", obj, shoff + 0x28)
    secs = [struct.unpack_from("<IIQQQQIIQQ", obj, shoff + i * 64) for i in range(shnum)]
    strtab = secs[shstrndx]

    def cstr(base, off):
        end = obj.index(b"\0", base + off)
        return obj[base + off:end].decode("latin-1")

    names = [cstr(strtab[4], s[0]) for s in secs]

    def sdata(i):
        s = secs[i]
        return b"" if s[1] == SHT_NOBITS else obj[s[4]:s[4] + s[5]]

    symtab = next((i for i, s in enumerate(secs) if s[1] == SHT_SYMTAB), None)
    xindex = {}
    for i, s in enumerate(secs):
        if s[1] == SHT_SYMTAB_SHNDX:
            xindex = {j: v for j, (v,) in enumerate(struct.iter_unpack("<I", sdata(i)))}
    syms = []
    if symtab is not None:
        st = secs[symtab]
        names_base = secs[st[6]][4]
        for j in range(st[5] // 24):
            st_name, st_info, _, st_shndx, _, _ = struct.unpack_from("<IBBHQQ", obj, st[4] + j * 24)
            if st_shndx == 0xFFFF:
                st_shndx = xindex.get(j, 0)
            syms.append((cstr(names_base, st_name) if st_name else "", st_info & 0xF, st_shndx))
    def target(sym):
        name, typ, shndx = syms[sym] if sym < len(syms) else ("", 0, 0)
        if name and typ != STT_SECTION:
            return ("S", hto.symbol_key(name))
        if 0 < shndx < len(secs):
            return ("#", target_section_name(names[shndx]))
        return ("#", "")

    relocs = collections.defaultdict(list)
    for i, s in enumerate(secs):
        if s[1] == SHT_RELA and s[7] < len(secs):
            for off, info, addend in struct.iter_unpack("<QQq", sdata(i)):
                relocs[s[7]].append((off, info & 0xFFFFFFFF, addend, info >> 32))
    out = {}
    for i, s in enumerate(secs):
        flags = s[2]
        if not flags & SHF_ALLOC or flags & SHF_MERGE or split_name(names[i]) is None:
            continue
        rs = [(off, t, add, target(sym)) for off, t, add, sym in relocs.get(i, [])]
        key = section_key(names[i], sdata(i), s[5], rs)
        if key:
            out[names[i]] = (key, s[5], flags)
    return out


def archive_keys(archives):
    """{(archive basename, member, section): (key, size)} over the archives."""
    out = {}
    for archive in archives:
        base = os.path.basename(archive)
        for member, body in ar_members(archive):
            for name, (key, size, _) in elf_data_keys(body).items():
                out.setdefault((base, member, name), (key, size))
    return out


def key_sizes(table):
    sizes = collections.Counter()
    for key, size in table.values():
        sizes[key] = max(sizes[key], size)
    return sizes


MAP_OUT = re.compile(r"^(\.[A-Za-z0-9_.$]+|perry_[a-z_]+)(\s+0x[0-9a-f]+\s+0x[0-9a-f]+)?\s*$")


def map_inputs(path, outputs=(".rodata", ".data.rel.ro")):
    """[(address, size, section, archive basename or "", member or file)] of a GNU ld map's input sections."""
    out, cur, pend = [], None, None
    for line in open(path, errors="replace"):
        line = line.rstrip("\n")
        if line and not line.startswith(" "):
            m = MAP_OUT.match(line)
            cur = m.group(1) if m else None
            pend = None
            continue
        if cur not in outputs:
            continue
        m = re.match(r"^ (\S+)\s+0x([0-9a-f]+)\s+0x([0-9a-f]+)\s+(\S.*)$", line)
        if not m:
            m2 = re.match(r"^ (\S+)$", line)
            if m2:
                pend = m2.group(1)
                continue
            m = re.match(r"^\s{10,}0x([0-9a-f]+)\s+0x([0-9a-f]+)\s+(\S.*)$", line)
            if not (m and pend):
                pend = None
                continue
            name, addr, size, origin = pend, m.group(1), m.group(2), m.group(3)
        else:
            name, addr, size, origin = m.groups()
        pend = None
        a, s = int(addr, 16), int(size, 16)
        if s == 0 or a == 0:
            continue
        am = re.match(r"^(.*\.a)\((.*)\)$", origin.strip())
        out.append((a, s, name, os.path.basename(am.group(1)) if am else "", am.group(2) if am else origin.strip()))
    out.sort()
    return out


def touched_granules(binary, args, cwd=None, timeout=7200):
    """The 8-byte granules of the binary's image that a run under lackey loaded or stored."""
    hi = 0
    out = subprocess.run(["readelf", "-lW", binary], check=True, capture_output=True, text=True).stdout
    for line in out.splitlines():
        f = line.split()
        if f and f[0] == "LOAD":
            hi = max(hi, int(f[2], 16) + int(f[5], 16))
    got = set()
    with tempfile.TemporaryDirectory(prefix="perry-hot-data-") as tmp:
        fifo = os.path.join(tmp, "trace")
        p = subprocess.Popen(["valgrind", "--tool=lackey", "--trace-mem=yes", f"--log-file={fifo}", binary, *args],
                             cwd=cwd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, stdin=subprocess.DEVNULL)
        try:
            p.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            p.kill()
            p.wait()
        if os.path.exists(fifo):
            with open(fifo, errors="replace") as f:
                for line in f:
                    if len(line) > 3 and line[0] == " " and line[1] in "LSM":
                        a, _, n = line[3:].partition(",")
                        a = int(a, 16)
                        if a < hi:
                            for g in range(a & ~7, a + max(int(n or 1), 1), 8):
                                got.add(g)
    return got


def resolve(granules, inputs, table):
    """{key: (size, touched bytes)} of the archive sections the granules fall in."""
    starts = [x[0] for x in inputs]
    import bisect
    hits = collections.Counter()
    for g in granules:
        i = bisect.bisect_right(starts, g) - 1
        if i >= 0 and g < inputs[i][0] + inputs[i][1]:
            hits[i] += 8
    out = {}
    for i, n in hits.items():
        _, size, name, base, member = inputs[i]
        hit = table.get((base, member, name))
        if hit:
            key = hit[0]
            s, t = out.get(key, (0, 0))
            out[key] = (max(s, size), t + n)
    return out


def cmd_record(a):
    binary = os.path.abspath(a.binary)
    table = archive_keys(a.archive)
    if a.granules:
        granules = [int(line, 16) for line in open(a.granules) if line.strip()]
    else:
        granules = touched_granules(binary, a.args)
    keys = resolve(granules, map_inputs(a.map), table)
    with open(a.output, "w") as o:
        for k, (size, touched) in sorted(keys.items()):
            o.write(f"{k} {size} {touched}\n")
    print(f"{len(keys)} runtime data sections read, {sum(s for s, _ in keys.values())} B -> {a.output}")
    return 0 if keys else 1


def load_recordings(paths):
    files = []
    for path in paths:
        if os.path.isdir(path):
            files += sorted(os.path.join(path, f) for f in os.listdir(path) if f.endswith(".keys"))
        else:
            files.append(path)
    progs = {}
    for f in files:
        keys = {}
        for line in open(f):
            p = line.split()
            if len(p) >= 2:
                keys[p[0]] = int(p[1])
        if keys:
            progs[os.path.basename(f)[:-5]] = keys
    return progs


def cmd_build(a):
    progs = load_recordings(a.recordings)
    held = load_recordings([a.held]) if a.held else {}
    for name in held:
        progs.pop(name, None)
    if not progs:
        sys.exit("no recordings")
    sizes = key_sizes(archive_keys(a.archive))
    n = len(progs)
    names = sorted(progs)
    count, bits = collections.Counter(), collections.Counter()
    for i, name in enumerate(names):
        for k in progs[name]:
            if k in sizes:
                count[k] += 1
                bits[k] |= 1 << i
    tiers = []
    rest = {k for k in count if count[k] >= a.min_programs}
    for label, floor, text in hto.TIERS:
        members = [k for k in rest if count[k] >= floor * n]
        rest -= set(members)
        by_count = sorted(members, key=lambda k: (-count[k], k))
        tiers.append((label, text, by_count if label == "core" else hto.cluster(by_count, bits)))
    listed = {k for _, _, ks in tiers for k in ks}
    report = [f"programs {n}; archive data sections {len(sizes)} keys, {sum(sizes.values())} B",
              "| class | sections | bytes (r / d) |", "|---|--:|--:|"]
    for label, text, ks in tiers:
        r = sum(sizes[k] for k in ks if k.startswith("r:"))
        d = sum(sizes[k] for k in ks if k.startswith("d:"))
        report.append(f"| {label}: {text} | {len(ks)} | {r} / {d} |")
    for label, group in (("train", progs), ("held out", held)):
        if not group:
            continue
        cover = sorted(sum(s for k, s in ks.items() if k in listed) / max(sum(s for k, s in ks.items() if k in sizes), 1)
                       for ks in group.values())
        report.append(f"{label}: {len(group)} programs; share of the read runtime data bytes that is listed: "
                      f"min {cover[0]:.3f} median {cover[len(cover) // 2]:.3f}")
    with open(a.output, "w") as o:
        o.write("# The runtime's default hot-data order (hot_data_order.rs). Generated by\n"
                "# scripts/hot_data_order.py build; one key per line (r: .rodata, d:\n"
                "# .data.rel.ro), placed at the front of its output section by tier.\n"
                f"# Recorded over {n} programs.\n")
        for label, text, ks in tiers:
            o.write(f"# tier {label}: {text}; {len(ks)} sections, {sum(sizes[k] for k in ks)} bytes\n")
            for k in ks:
                o.write(k + "\n")
    text = "\n".join(report)
    print(text)
    if a.report:
        open(a.report, "w").write(text + "\n")
    return 0


def cmd_check(a):
    if not a.perry and not (a.names_only and a.runtime_dir):
        sys.exit("check needs --perry, or --names-only with --runtime-dir")
    keys, core = hto.read_order(a.order)
    rt = a.runtime_dir or os.path.dirname(os.path.abspath(a.perry))
    archives = [p for p in (os.path.join(rt, n) for n in ("libperry_runtime.a", "libperry_stdlib.a")) if os.path.exists(p)]
    if not archives:
        sys.exit(f"no runtime archive in {rt}")
    table = archive_keys(archives)
    sizes = key_sizes(table)
    failed = False
    missing = [(i, k) for i, k in enumerate(keys) if k not in sizes]
    core_missing = [k for i, k in missing if i < core]
    pct = 100.0 * len(missing) / max(len(keys), 1)
    print(f"names: {len(keys) - len(missing)} of {len(keys)} listed data sections exist; missing {len(missing)} "
          f"({pct:.2f}%), of the core tier {len(core_missing)} of {core}")
    for k in core_missing[:40]:
        print(f"  missing core section: {k}")
    if len(core_missing) > a.max_core_missing:
        failed = True
        print(f"FAIL: {len(core_missing)} core-tier data sections no longer exist (allowed {a.max_core_missing}): rebuild the list.")
    if pct > a.max_missing_pct:
        failed = True
        print(f"FAIL: {pct:.2f}% of the listed data sections no longer exist (allowed {a.max_missing_pct}%): rebuild the list.")
    if a.names_only:
        print("hot-data order (names):", "STALE" if failed else "ok")
        return 1 if failed else 0
    listed = set(keys)
    programs = [(p.split("::")[0], p.split("::")[1].split() if "::" in p else []) for p in a.program]
    with tempfile.TemporaryDirectory(prefix="perry-hot-data-") as tmp:
        if not programs:
            with open(os.path.join(tmp, "smoke.ts"), "w") as o:
                o.write(hto.SMOKE)
            programs = [(os.path.join(tmp, "smoke.ts"), [])]
        if subprocess.run(["sh", "-c", "command -v valgrind"], capture_output=True).returncode != 0 and not a.require_coverage:
            print("coverage: skipped (no valgrind)")
            programs = []
        for index, (source, args) in enumerate(programs):
            binary = os.path.join(tmp, f"prog{index}")
            link_map = binary + ".map"
            env = dict(os.environ, PERRY_KEEP_SYMBOLS="1", PERRY_NO_CACHE="1", PERRY_RUNTIME_DIR=rt, PERRY_LINK_MAP=link_map)
            env.pop("PERRY_HOT_DATA_ORDER", None)
            if os.uname().machine == "x86_64":
                env.setdefault("PERRY_TARGET_CPU", "x86-64-v3")
            c = subprocess.run([a.perry, "compile", os.path.basename(source), "-o", binary, *a.compile_arg],
                               cwd=os.path.dirname(os.path.abspath(source)), env=env, capture_output=True, text=True)
            if c.returncode != 0 or not os.path.exists(link_map):
                print(f"FAIL: compiling {source} (with a link map):\n{c.stdout[-2000:]}{c.stderr[-2000:]}")
                failed = True
                continue
            syms = hto.all_symbols(binary)
            runs = [(syms.get(f"__perry_rt_hot_{n}_start"), syms.get(f"__perry_rt_hot_{n}_end")) for n in ("rodata", "relro")]
            runs = [r for r in runs if r[0] is not None and r[1] is not None]
            if not runs:
                print(f"FAIL: {source}: the link placed no hot data run (no __perry_rt_hot_rodata_start)")
                failed = True
                continue
            inputs = map_inputs(link_map)
            read = resolve(touched_granules(binary, args, os.path.dirname(os.path.abspath(source))), inputs, table)
            placed = {}
            for addr, size, name, base, member in inputs:
                hit = table.get((base, member, name))
                if hit:
                    placed.setdefault(hit[0], addr)
            inside = outside = 0
            strays = []
            for k, (size, _) in read.items():
                at = placed.get(k, 0)
                if any(lo <= at < hi for lo, hi in runs):
                    inside += size
                else:
                    outside += size
                    strays.append((size, k, k in listed))
            total = inside + outside
            out_pct = 100.0 * outside / max(total, 1)
            print(f"coverage: {os.path.basename(source)} {' '.join(args)}: read runtime data {total} B in {len(read)} sections; "
                  f"outside the hot runs {outside} B = {out_pct:.2f}% ({len(strays)} sections)")
            for size, k, was_listed in sorted(strays, reverse=True)[:8 if not a.verbose else None]:
                print(f"  outside: {size:6d} {k}{' (listed)' if was_listed else ''}")
            if total == 0 or out_pct > a.max_outside_pct:
                failed = True
                print(f"FAIL: more than {a.max_outside_pct}% of the read runtime data is outside the hot runs: rebuild the list.")
    print("hot-data order:", "STALE" if failed else "ok")
    return 1 if failed else 0


# (section name, bytes, size, relocations, key): the vectors hot_data_order.rs tests too.
KEY_VECTORS = [
    (".rodata._RNvNtNtCsXbwMRDrjNy_13perry_runtime2gc5types18GC_TYPE_INFO", b"", 4096, [],
     "r:_RNvNtNtCs_13perry_runtime2gc5types18GC_TYPE_INFO"),
    (".data.rel.ro.local._mi_theap_empty", b"", 8096, [], "d:_mi_theap_empty"),
    (".rodata.unlikely._RNvNtCsAB_5crate3TAB", b"", 4, [], "r:_RNvNtCs_5crate3TAB"),
    (".rodata..Lanon.066df2a3b4d7718a1445f540d0b35fcf.717", b"length", 6, [], "r:#0cc57c9be00b204b"),
    (".data.rel.ro..Lanon.066df2a3b4d7718a1445f540d0b35fcf.12", b"\0" * 16, 16,
     [(8, 1, 0, ("S", "js_string_concat")), (0, 1, 3, ("#", ".rodata"))], "d:#b9adc86cf57ea99a"),
    (".rodata", b"x", 1, [], None),
    (".rodata.perry_fn_info_x::y", b"", 1, [], None),
]


def selftest():
    bad = 0
    for name, data, size, relocs, want in KEY_VECTORS:
        got = section_key(name, data, size, relocs)
        if got != want:
            bad += 1
            print(f"FAIL key({name})\n  got  {got}\n  want {want}")
    print("selftest", "FAILED" if bad else "ok", f"({len(KEY_VECTORS)} vectors)")
    return 1 if bad else 0


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    r = sub.add_parser("record")
    r.add_argument("--map", required=True, help="the GNU ld link map of BIN (PERRY_LINK_MAP)")
    r.add_argument("--archive", action="append", required=True)
    r.add_argument("--granules", help="the touched 8-byte granules (hex, one per line) of an earlier run instead of running BIN")
    r.add_argument("-o", "--output", required=True)
    r.add_argument("binary")
    r.add_argument("args", nargs=argparse.REMAINDER)
    b = sub.add_parser("build")
    b.add_argument("--archive", action="append", required=True)
    b.add_argument("-o", "--output", default=ORDER)
    b.add_argument("--report")
    b.add_argument("--min-programs", type=int, default=2)
    b.add_argument("--held")
    b.add_argument("recordings", nargs="+")
    c = sub.add_parser("check")
    c.add_argument("--perry")
    c.add_argument("--names-only", action="store_true")
    c.add_argument("--runtime-dir")
    c.add_argument("--order", default=ORDER)
    c.add_argument("--program", action="append", default=[])
    c.add_argument("--compile-arg", action="append", default=[])
    c.add_argument("--max-core-missing", type=int, default=0)
    c.add_argument("--max-missing-pct", type=float, default=2.0)
    c.add_argument("--max-outside-pct", type=float, default=10.0)
    c.add_argument("--require-coverage", action="store_true")
    c.add_argument("-v", "--verbose", action="store_true")
    sub.add_parser("selftest")
    a = ap.parse_args()
    if a.cmd == "selftest":
        return selftest()
    return {"record": cmd_record, "build": cmd_build, "check": cmd_check}[a.cmd](a)


if __name__ == "__main__":
    sys.exit(main())
