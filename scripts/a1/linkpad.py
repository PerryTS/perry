#!/usr/bin/env python3
"""scripts/a1/linkpad.py: link-pad control for startup A/Bs (nightly/CW/STARTUP_RULE_2026-10-28.md).

Some startup rows follow addresses, not code: when one arm's .text/.rodata/... is smaller, every later
section and the heap start lower (mimalloc page map, address-sorted ranges, libc alignment). The control
pads each arm so both get the same end address for every paddable output section, which makes every
later section, the segment starts and the heap start coincide. A pad is an object of unreferenced
SHF_GNU_RETAIN input sections (so --gc-sections keeps it) linked last through PERRY_EXTRA_LINK_ARGS; it
lands at the end of its output section and nothing executes or reads it.

  linkpad.py plan  <main-bin> <head-bin> <pads.json> <pad-dir>
      Read both binaries (already carrying the pads in pads.json, if any), update pads.json so every
      paddable section ends at the same address in both arms, write <pad-dir>/<arm>-<sha>.o for each arm
      with a nonzero pad and print "CONVERGED" when no pad changed, else "PADS main=<obj|-> head=<obj|->".
  linkpad.py check <main-bin> <head-bin>
      Print the per-section layout comparison; exit 1 unless every paddable section ends at the same
      address, all PT_LOAD segments start at the same address and the image ends at the same address.

Only sections whose bytes are reached through pointers are padded. The runtime walks perry_* sections
(__start_/__stop_), .eh_frame and stack maps record by record, so those are never padded: a size
difference there is taken up by the next paddable section of the same segment.
"""
import hashlib, json, os, subprocess, sys

# input section name and assembler flags/type for each paddable output section
PADDABLE = {
    ".text": ('.text.lpad', '"axR",@progbits', '0xcc'),
    ".rodata": ('.rodata.lpad', '"aR",@progbits', '0'),
    ".gcc_except_table": ('.gcc_except_table.lpad', '"aR",@progbits', '0'),
    ".data.rel.ro": ('.data.rel.ro.lpad', '"awR",@progbits', '0'),
    ".data": ('.data.lpad', '"awR",@progbits', '0'),
    ".bss": ('.bss.lpad', '"awR",@nobits', None),
}


def sections(path):
    """Allocated sections in address order: [(name, addr, size)]."""
    out = subprocess.run(["readelf", "-SW", path], check=True, capture_output=True, text=True).stdout
    secs = []
    for line in out.splitlines():
        line = line.strip()
        if not line.startswith("[") or "]" not in line:
            continue
        f = line.split("]", 1)[1].split()
        if len(f) < 7:
            continue
        try:
            name, addr, size, flags = f[0], int(f[2], 16), int(f[4], 16), f[6]
        except ValueError:
            continue
        if "A" in flags and addr:
            secs.append((name, addr, size))
    return sorted(secs, key=lambda s: (s[1], s[0]))


def loads(path):
    out = subprocess.run(["readelf", "-lW", path], check=True, capture_output=True, text=True).stdout
    return [(int(f[2], 16), int(f[5], 16), f[6]) for f in (l.split() for l in out.splitlines())
            if f and f[0] == "LOAD"]


def notes(path):
    """The GNU property note: a pad object must not change the linked features (IBT/SHSTK, ISA)."""
    r = subprocess.run(["readelf", "-x", ".note.gnu.property", path], capture_output=True, text=True)
    # hex dump lines start with the section address, which a pad may move: compare the bytes only
    return [l.split()[1:5] for l in r.stdout.splitlines() if l.strip().startswith("0x")]


def paired(m, h):
    names_m, names_h = [s[0] for s in m], [s[0] for s in h]
    if names_m != names_h:
        sys.exit(f"section order differs:\n main {names_m}\n head {names_h}")
    return list(zip(m, h))


def write_pad(pads, arm, pad_dir):
    want = {k: v for k, v in pads.get(arm, {}).items() if v > 0}
    if not want:
        return "-"
    src = ""
    for out_sec in PADDABLE:
        if out_sec in want:
            name, flags, fill = PADDABLE[out_sec]
            src += f".section {name},{flags}\n" + (f".space {want[out_sec]},{fill}\n" if fill else f".zero {want[out_sec]}\n")
    src += '.section .note.GNU-stack,"",@progbits\n'
    obj = os.path.join(pad_dir, f"{arm}-{hashlib.sha256(src.encode()).hexdigest()[:16]}.o")
    if not os.path.exists(obj):
        os.makedirs(pad_dir, exist_ok=True)
        s = obj[:-2] + ".S"
        open(s, "w").write(src)
        subprocess.run(["cc", "-c", s, "-o", obj], check=True)
    return obj


def plan(main_bin, head_bin, pads_json, pad_dir):
    pads = json.load(open(pads_json)) if os.path.exists(pads_json) else {"main": {}, "head": {}}
    changed = False
    # Walk the sections in address order. Until this round changes a pad, the observed end difference of a
    # paddable section is exact. After that the binaries no longer show the new layout, so the difference
    # is modelled: a padded section ends equal, and every later section starts equal plus the size
    # differences of the non-paddable sections in between (alignment can absorb some of those; the next
    # round sees what the linker did and corrects it).
    carry = 0
    for (n, am, sm), (_, ah, sh) in paired(sections(main_bin), sections(head_bin)):
        if n not in PADDABLE:
            carry += sh - sm
            continue
        r = (ah + sh) - (am + sm) if not changed else carry + sh - sm
        carry = 0
        if r == 0:
            continue
        big, small = ("head", "main") if r > 0 else ("main", "head")
        r = abs(r)
        # keep pads minimal: first take back the longer arm's own pad, then pad the shorter arm
        back = min(r, pads[big].get(n, 0))
        pads[big][n] = pads[big].get(n, 0) - back
        pads[small][n] = pads[small].get(n, 0) + r - back
        changed = True
    json.dump(pads, open(pads_json, "w"), indent=1, sort_keys=True)
    if not changed:
        print("CONVERGED")
    else:
        print(f"PADS main={write_pad(pads, 'main', pad_dir)} head={write_pad(pads, 'head', pad_dir)}")


def check(main_bin, head_bin):
    ok = True
    print(f"{'section':<24}{'addr main':>12}{'addr head':>12}{'size main':>11}{'size head':>11}")
    for (n, am, sm), (_, ah, sh) in paired(sections(main_bin), sections(head_bin)):
        mark = ""
        if n in PADDABLE and am + sm != ah + sh:
            mark, ok = "  END DIFFERS", False
        elif am != ah:
            mark = "  start moved (a non-paddable section before it differs in size)"
        if am != ah or sm != sh or mark:
            print(f"{n:<24}{am:>12x}{ah:>12x}{sm:>11x}{sh:>11x}{mark}")
    lm, lh = loads(main_bin), loads(head_bin)
    for (vm, mm, fm), (vh, mh, fh) in zip(lm, lh):
        if vm != vh or fm != fh:
            print(f"LOAD {fm} {vm:x} vs {fh} {vh:x}  START DIFFERS")
            ok = False
    em, eh = lm[-1][0] + lm[-1][1], lh[-1][0] + lh[-1][1]
    print(f"image end (heap start side): main {em:x} head {eh:x}" + ("" if em == eh else "  DIFFERS"))
    if len(lm) != len(lh) or em != eh:
        ok = False
    if notes(main_bin) != notes(head_bin):
        print(".note.gnu.property DIFFERS")
        ok = False
    print("LAYOUT-EQUAL" if ok else "LAYOUT-DIFFERS")
    return 0 if ok else 1


if __name__ == "__main__":
    cmd = sys.argv[1] if len(sys.argv) > 1 else ""
    if cmd == "plan" and len(sys.argv) == 6:
        plan(*sys.argv[2:])
    elif cmd == "check" and len(sys.argv) == 4:
        sys.exit(check(*sys.argv[2:]))
    else:
        sys.exit(__doc__)
