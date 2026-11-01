#!/usr/bin/env python3
"""Realm link-graph gate (WRT3p-A1, DESIGN §3.2 items 9 and 10).

Compiles hello on ELF with `PERRY_REALM_REACH=1` and `-Wl,--emit-relocs`, then
proves from the linked binary:

1. R is the set of `%Layout:<NAME>%` names in the compiler's
   `[realm-reach-json]` line.
2. `nm`: every defined (global) `perry_runtime_layout_<NAME>` descriptor is
   in R; no live reference (`w`, a weak reference the link kept with no
   definition) names a layout in R, so every R layout the program can reach
   at a live birth is defined. An R layout the link names nowhere was dropped
   with its dead births (`--gc-sections`); the codegen test
   `program_runtime_layouts_are_exactly_r_and_equal_the_declared_facts` pins
   that the compiler emits exactly R. Each local `<descriptor>_list*` cell
   belongs to a defined descriptor in R.
3. `nm`: `perry_test_layout_descriptors` (the marker of the test-only
   descriptor definitions, `cfg(test)` / feature `test-layout-descriptors`)
   is absent, so a release program carries none of them.
4. Retained static relocations tie each descriptor to its own cells: its
   `keys` and `index` fields are relocated, and its `list` field is either
   null (a symbol-keyed layout before PR B) or points into `perry_realm` at a
   list no other descriptor names.

`--self-test` relinks hello with a planted out-of-R descriptor symbol and with
the test marker, and requires the gate to fail on each (a gate that cannot
fail proves nothing).
"""
import argparse
import json
import os
import re
import subprocess
import sys
import tempfile

HELLO = 'console.log("Hello, world!");\n'
PREFIX = "perry_runtime_layout_"
MARKER = "perry_test_layout_descriptors"
# PerryLayoutDesc on 64-bit ELF (codegen layout_desc::DESC_TY):
# { i32, i16, i8, i8, i32, i32, i64, ptr proto, ptr keys, i32, i32, ptr index,
#   i32, i32, ptr methods, i32, i32, ptr list, i64, [32 x i8] }
DESC_SIZE = 128
KEYS_OFF, INDEX_OFF, LIST_OFF = 32, 48, 80


def tool(name):
    for candidate in (f"llvm-{name}", f"llvm-{name}-22", name):
        try:
            subprocess.run([candidate, "--version"], capture_output=True, check=True)
            return candidate
        except (OSError, subprocess.CalledProcessError):
            continue
    sys.exit(f"realm_wgraph_gate: no {name} on PATH")


def compile_hello(perry, out, extra_link=""):
    os.makedirs(out, exist_ok=True)
    src = os.path.join(out, "hello.ts")
    with open(src, "w") as f:
        f.write(HELLO)
    binary = os.path.join(out, "hello")
    env = dict(os.environ)
    env["PERRY_REALM_REACH"] = "1"
    env["PERRY_NO_CACHE"] = "1"
    env["PERRY_EXTRA_LINK_ARGS"] = ("-Wl,--emit-relocs " + extra_link).strip()
    run = subprocess.run([perry, "compile", src, "-o", binary], env=env,
                         capture_output=True, text=True, cwd=out)
    with open(os.path.join(out, "compile.log"), "w") as f:
        f.write(run.stdout + run.stderr)
    if run.returncode != 0:
        sys.exit(f"realm_wgraph_gate: hello failed to compile (see {out}/compile.log)")
    lines = [l for l in run.stderr.splitlines() if l.startswith("[realm-reach-json] ")]
    if len(lines) != 1:
        sys.exit("realm_wgraph_gate: expected one [realm-reach-json] line")
    reach = json.loads(lines[0].split(" ", 1)[1])
    r = {m.group(1) for n in reach["names"] if (m := re.fullmatch(r"%Layout:(.+)%", n))}
    return binary, r


def symbols(binary):
    """name -> (address, size, nm type letter)"""
    out = subprocess.run([tool("nm"), "-S", binary], capture_output=True, text=True, check=True).stdout
    table = {}
    for line in out.splitlines():
        parts = line.split()
        if len(parts) == 4:
            table[parts[3]] = (int(parts[0], 16), int(parts[1], 16), parts[2])
        elif len(parts) == 3:
            table[parts[2]] = (int(parts[0], 16), 0, parts[1])
        elif len(parts) == 2:
            table[parts[1]] = (0, 0, parts[0])
    return table


def layout_symbols(syms):
    """(defined global descriptors, live undefined references, local cells) by layout name"""
    defined, referenced, cells = set(), set(), {}
    for n, (_, _, kind) in syms.items():
        if not n.startswith(PREFIX):
            continue
        name = n[len(PREFIX):]
        if kind in "UVvw":
            referenced.add(name)
        elif kind.isupper():
            defined.add(name)
        else:
            cells[name] = n
    return defined, referenced, cells


def sections(binary):
    out = subprocess.run([tool("readelf"), "-S", "-W", binary], capture_output=True, text=True, check=True).stdout
    table = {}
    for line in out.splitlines():
        m = re.match(r"\s*\[\s*\d+\]\s+(\S+)\s+\S+\s+([0-9a-f]+)\s+[0-9a-f]+\s+([0-9a-f]+)", line)
        if m:
            table[m.group(1)] = (int(m.group(2), 16), int(m.group(3), 16))
    return table


def static_relocs(binary):
    """offset -> target address, from the retained (non-dynamic) relocation sections."""
    out = subprocess.run([tool("readelf"), "-r", "-W", binary], capture_output=True, text=True, check=True).stdout
    relocs, keep = {}, False
    for line in out.splitlines():
        head = re.match(r"Relocation section '([^']+)'", line)
        if head:
            keep = head.group(1) not in (".rela.dyn", ".rela.plt")
            continue
        if not keep:
            continue
        m = re.match(r"([0-9a-f]+)\s+[0-9a-f]+\s+(R_X86_64_64|R_AARCH64_ABS64)\s+([0-9a-f]+)\s+(.*?)\s*([+-])\s*([0-9a-f]+)\s*$", line)
        if m:
            addend = int(m.group(6), 16) * (1 if m.group(5) == "+" else -1)
            relocs[int(m.group(1), 16)] = int(m.group(3), 16) + addend
    return relocs


def check(binary, r):
    """Returns the list of failures (empty = pass)."""
    failures = []
    syms = symbols(binary)
    defined, referenced, cells = layout_symbols(syms)
    for name in sorted(defined - r):
        failures.append(f"{PREFIX}{name} is linked but R does not reach it")
    for name in sorted(referenced & r):
        failures.append(f"{PREFIX}{name} is in R and referenced, but not defined")
    for cell, symbol in sorted(cells.items()):
        owner = next((d for d in defined & r if re.fullmatch(re.escape(d) + r"_(list|list_attrs|keys|index|prototype)", cell)), None)
        if owner is None:
            failures.append(f"{symbol} belongs to no defined descriptor in R")
    if MARKER in syms:
        failures.append(f"{MARKER} is linked: test-only layout descriptors are in a release program")
    secs = sections(binary)
    realm = secs.get("perry_realm")
    relocs = static_relocs(binary)
    if not relocs:
        failures.append("no static relocations retained (link without -Wl,--emit-relocs?)")
        return failures
    lists = {}
    for name in sorted(defined & r):
        addr, size, _ = syms[PREFIX + name]
        if size != DESC_SIZE:
            failures.append(f"{PREFIX}{name}: size {size}, not a descriptor ({DESC_SIZE})")
            continue
        for field, off in (("keys", KEYS_OFF), ("index", INDEX_OFF)):
            if addr + off not in relocs:
                failures.append(f"{PREFIX}{name}: its {field} field names no cell")
        target = relocs.get(addr + LIST_OFF)
        if target is None:
            continue  # a symbol-keyed layout: no image list before PR B
        if realm is None or not (realm[0] <= target < realm[0] + realm[1]):
            failures.append(f"{PREFIX}{name}: its list is outside perry_realm")
        if target in lists:
            failures.append(f"{PREFIX}{name} and {PREFIX}{lists[target]} share one list")
        lists[target] = name
    return failures


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--perry", required=True)
    ap.add_argument("--output", default="target/realm-wgraph")
    ap.add_argument("--self-test", action="store_true")
    args = ap.parse_args()
    perry = os.path.abspath(args.perry)
    out = os.path.abspath(args.output)
    binary, r = compile_hello(perry, os.path.join(out, "hello"))
    if not r:
        sys.exit("realm_wgraph_gate: R reaches no runtime layout (the reach line changed?)")
    failures = check(binary, r)
    run = subprocess.run([binary], capture_output=True, text=True)
    if run.returncode != 0 or run.stdout != "Hello, world!\n":
        failures.append(f"hello did not run: rc={run.returncode} stdout={run.stdout!r}")
    defined, referenced, _ = layout_symbols(symbols(binary))
    print(f"realm_wgraph_gate: R has {len(r)} runtime layouts; hello defines {len(defined)} "
          f"({len(r - defined - referenced)} in R dropped by the link with their dead births); "
          f"{len(referenced)} live weak references outside R stay undefined")
    for f in failures:
        print(f"FAIL {f}")
    if failures:
        return 1
    if args.self_test:
        _, referenced, _ = layout_symbols(symbols(binary))
        outside = sorted(referenced - r)
        if not outside:
            print("FAIL self-test: no out-of-R layout name to plant")
            return 1
        plants = {
            "out-of-R": f"-Wl,--defsym={PREFIX}{outside[0]}=main",
            "test-marker": f"-Wl,--defsym={MARKER}=main",
        }
        for label, link in plants.items():
            planted, pr = compile_hello(perry, os.path.join(out, f"plant-{label}"), link)
            got = check(planted, pr)
            if not got:
                print(f"FAIL self-test: the gate passed a hello with a planted {label} symbol")
                return 1
            print(f"self-test {label}: gate fails as required ({got[0]})")
    print("realm_wgraph_gate: PASS")
    return 0


if __name__ == "__main__":
    sys.exit(main())
