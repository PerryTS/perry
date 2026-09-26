#!/usr/bin/env python3
"""Per-crate size attribution from `nm -C --size-sort -S` output (text/data symbols).

usage: attribute.py <nm-output[.gz]> [--top N] [--json]
A symbol is attributed to the first non-std crate path in its demangled name
(so `core::ptr::drop_in_place<tokio::net::TcpStream>` counts as tokio); C
symbols are bucketed by well-known prefixes. Sizes are symbol sizes (st_size),
which under-count padding/unwind tables — use for shares, not for totals.
"""
import gzip, json, re, sys, collections

STD = {"core", "alloc", "std", "compiler_builtins", "hashbrown", "memchr", "panic_unwind", "panic_abort", "gimli", "addr2line", "rustc_demangle", "miniz_oxide", "object", "adler2", "std_detect"}
C_PREFIX = [(r"^_?aws_lc_", "aws-lc (C)"), (r"^_?ring_core_", "ring (C)"), (r"^_?(mi_|_mi_)", "mimalloc (C)"),
            (r"^_?(js_|perry_|__perry)", "perry exports (js_*/perry_*)"), (r"^_?(sqlite3)", "sqlite (C)"),
            (r"^_?(ZSTD|zstd|HUF_|FSE_)", "zstd (C)"), (r"^_?(deflate|inflate|crc32|adler32|zlibVersion|gz)", "zlib (C)"),
            (r"^_?(__rust|rust_)", "rust rt"), (r"^_?(\.L|ltmp)", "local labels")]
# only the FIRST segment of a path counts: `core::slice::iter` is core, never `slice`
PATH = re.compile(r"(?:^|(?<=[<\s(&*,\[;{]))([a-z_][a-z0-9_]*)::")

def crate_of(name):
    for m in PATH.finditer(name):
        c = m.group(1)
        if c in STD or c in ("impl", "dyn", "fn", "as", "mut", "const"): continue
        return c
    if "::" in name:
        m = PATH.search(name); return m.group(1) if m else "?"
    for pat, lab in C_PREFIX:
        if re.search(pat, name): return lab
    return "other/C/unmangled"

def main():
    path = sys.argv[1]
    op = gzip.open if path.endswith(".gz") else open
    tot = collections.Counter(); cnt = collections.Counter()
    with op(path, "rt", errors="replace") as f:
        for line in f:
            parts = line.rstrip("\n").split(" ", 3)
            if len(parts) < 4: continue
            try: size = int(parts[1], 16)
            except ValueError: continue
            if parts[2] not in "tTdDbBrRwWvV": continue
            c = crate_of(parts[3]); tot[c] += size; cnt[c] += 1
    top = int(sys.argv[sys.argv.index("--top") + 1]) if "--top" in sys.argv else 40
    if "--json" in sys.argv:
        print(json.dumps({c: {"bytes": b, "symbols": cnt[c]} for c, b in tot.most_common()}))
    else:
        s = sum(tot.values())
        print(f"total attributed {s} bytes")
        for c, b in tot.most_common(top): print(f"{b:>10} {100*b/s:5.1f}% {cnt[c]:>6}  {c}")

main()
