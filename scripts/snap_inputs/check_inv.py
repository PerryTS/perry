#!/usr/bin/env python3
"""Cut-point inventory fixtures.

  check_inv.py last-init PROBE_REPORT          print the id of the last init the program entered
  check_inv.py check NAME REPORT BINARY        assert the inventory of fixture NAME (i1 i2 i3)
  check_inv.py print REPORT BINARY             totals and the symbolised word list

REPORT is the census output prefix; the inventory is REPORT.inventory.tsv.
"""
import re, subprocess, sys, bisect


def last_init(probe):
    best = 0
    for l in open(probe + ".inputs.tsv").read().splitlines()[1:]:
        f = l.split("\t")
        if len(f) > 6 and f[4] == "init_enter":
            best = max(best, int(f[2]))
    return best


def load(report):
    cells, types, words, totals, wc = [], [], [], {}, []
    head = ""
    for l in open(report + ".inventory.tsv"):
        f = l.rstrip("\n").split("\t")
        if f[0] == "#inventory":
            head = l.strip()
        elif f[0] == "cell":
            cells.append(dict(t=int(f[1]), name=f[2], form=f[3], init=f[4], n=int(f[5]), b=int(f[6])))
        elif f[0] == "#type":
            types.append(dict(t=int(f[1]), name=f[2], form=f[3], n=int(f[4]), b=int(f[5])))
        elif f[0] == "word":
            words.append(dict(region=f[1], addr=int(f[2], 16), off=int(f[3], 16), old=f[4], new=f[5], cls=f[6]))
        elif f[0] == "#words":
            wc.append((f[1], f[2], int(f[3])))
        elif f[0] == "#total":
            totals = dict(p.split("=") for p in f[1:])
    return head, cells, types, words, wc, totals


def symbols(binary):
    """(sorted data symbols, tls symbols) of the binary."""
    data = []
    for l in subprocess.run(["nm", "-n", "-S", "--defined-only", binary], capture_output=True, text=True).stdout.splitlines():
        p = l.split()
        if len(p) == 4 and p[2] in "bBdDrR":
            data.append((int(p[0], 16), int(p[1], 16), p[3]))
    tls = []
    for l in subprocess.run(["readelf", "-sW", binary], capture_output=True, text=True).stdout.splitlines():
        p = l.split()
        if len(p) >= 8 and p[3] == "TLS":
            tls.append((int(p[1], 16), int(p[2]), p[7]))
    tls.sort()
    return data, tls


def name_of(w, syms):
    data, tls = syms
    tab, key = (data, w["addr"]) if w["region"] == "data" else (tls, w["off"])
    i = bisect.bisect_right([s[0] for s in tab], key) - 1
    if i < 0:
        return "?"
    s = tab[i]
    off = key - s[0]
    return f"{s[2]}+{off:#x}" if off < max(s[1], 8) else f"?(after {s[2]})"


def main():
    cmd = sys.argv[1]
    if cmd == "last-init":
        print(last_init(sys.argv[2]))
        return 0
    if cmd == "print":
        report, binary = sys.argv[2:4]
        head, cells, types, words, wc, totals = load(report)
        syms = symbols(binary)
        print(head)
        for t in types:
            print("type", t["name"], t["form"], t["n"], t["b"])
        for r, c, n in wc:
            print("words", r, c, n)
        for w in words:
            print("word", w["region"], w["cls"], name_of(w, syms))
        print("total", totals)
        return 0
    name, report, binary = sys.argv[2:5]
    head, cells, types, words, wc, totals = load(report)
    syms = symbols(binary)
    names = [(w, name_of(w, syms)) for w in words]
    fails = []

    def need(c, msg):
        if not c:
            fails.append(msg)

    def ty(n):
        return [t for t in types if t["name"] == n]

    need("collected" in head and "not-collected" not in head, "run was not held: " + head)
    need(int(totals.get("cell_n", 0)) > 0, "no cell rows")
    if name == "i1":
        need(any(t["form"] == "cell" and t["n"] >= 1 for t in ty("symbol")), "no symbol cell")
        need(any(t["form"] == "cell" and t["n"] >= 1 for t in ty("map_store")), "no map store cell")
        data = [n for w, n in names if w["region"] == "data"]
        need(any("6symbol" in n for n in data), "no changed data word names the symbol statics")
        need(any("class_registry" in n for n in data), "no changed data word names the class registry statics")
        need(any(w["cls"] == "gc" for w, n in names), "no changed word points at a program cell")
    elif name == "i2":
        need(any(t["form"] == "native" and t["name"] == "regex_program" for t in types), "the compiled regexp program is not native")
        need(not any(t["form"] == "cell" and t["name"].startswith("native") for t in types), "a native type is marked imageable")
    elif name == "i3":
        need(any(w["region"] == "tls-program" for w, n in names), "no changed program TLS word")
        need(any(t["name"] == "promise" and t["n"] >= 1 for t in types), "no promise cell")
    else:
        fails.append("unknown fixture " + name)
    print(f"{name} ok" if not fails else f"{name} FAIL: " + "; ".join(fails))
    return 1 if fails else 0


sys.exit(main())
