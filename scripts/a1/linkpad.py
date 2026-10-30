#!/usr/bin/env python3
"""scripts/a1/linkpad.py: link-pad control for startup A/Bs (nightly/CW/STARTUP_RULE_2026-10-28.md).

Some startup rows follow addresses, not code: when one arm's compiled code or data is smaller, every
runtime byte after it (and the heap) starts lower (mimalloc page map, address-sorted ranges, libc
alignment, ld.so symbol lookup order). The control pads each arm so the runtime archive's bytes sit at
the same address in both arms.

Two pad objects per arm, both made of unreferenced SHF_GNU_RETAIN input sections nothing executes or reads:

  mid pad  (PERRY_LINK_PAD)         placed by perry right after the compiled objects and before the runtime
                                    archive on every final link. The linker lays input sections out in
                                    command-line order, so it moves the runtime's part of each output section.
                                    It defines a marker symbol at the end of each padded section
                                    (__lpad_mid_<sec>); the marker is where the runtime's bytes start.
  end pad  (PERRY_EXTRA_LINK_ARGS)  linked last. Only matters when the runtime itself differs in size between
                                    the arms: it makes every paddable section end, and the image end (heap
                                    start), coincide anyway.

  linkpad.py zero <pad-dir>
      Write the mid pad with no extra bytes (markers and the .eh_frame filler only) and print its path.
      Both arms are linked with it first, so every marker exists.
  linkpad.py plan  <main-bin> <head-bin> <pads.json> <pad-dir>
      Read both binaries (already carrying the pads in pads.json, if any), update pads.json so every
      marker and every paddable section end coincide, write <pad-dir>/<arm>-<mid|end>-<sha>.o for each
      arm and print "CONVERGED" when no pad changed, else "PADS main=<mid>,<end> head=<mid>,<end>"
      ("-" for no end pad).
  linkpad.py check <main-bin> <head-bin> [<runtime-lib.a>]
      Print the layout comparison and, per output section, how many symbols defined in both binaries (and
      in the runtime archive, when given) sit at the same address; exit 1 unless all markers and paddable
      section ends coincide, all PT_LOAD segments start at the same address and the image ends at the same
      address.

.eh_frame is padded with one FDE (a retained function in .text.lpad); its marker is the end of that FDE, found
through the .eh_frame_hdr search table. Sections the runtime walks record by record (perry_*, stack maps)
and the linker-generated .eh_frame_hdr are never padded: their size differences are taken up by the next
padded section, so the runtime bytes after them still coincide, but the runtime records inside them move
with the compiled records in front of them.
"""
import hashlib, json, os, struct, subprocess, sys

# output section -> (mid input section, flags, fill, marker) and (end input section)
PADDABLE = {
    ".text": ('.text.lpad', '"axR",@progbits', '0xcc', '__lpad_mid_text', '.text.lpade'),
    ".rodata": ('.rodata.lpad', '"aR",@progbits', '0', '__lpad_mid_rodata', '.rodata.lpade'),
    ".gcc_except_table": ('.gcc_except_table.lpad', '"aR",@progbits', '0', '__lpad_mid_gcc_except_table',
                          '.gcc_except_table.lpade'),
    ".data.rel.ro": ('.data.rel.ro.lpad', '"awR",@progbits', '0', '__lpad_mid_data_rel_ro', '.data.rel.ro.lpade'),
    ".data": ('.data.lpad', '"awR",@progbits', '0', '__lpad_mid_data', '.data.lpade'),
    ".bss": ('.bss.lpad', '"awR",@nobits', None, '__lpad_mid_bss', '.bss.lpade'),
}
# SHF_MERGE blobs inside .rodata: the linker merges every input of one (flags, entsize) class into the blob of
# the first, compiled, input, so the runtime's constants and strings move whenever the compiled ones differ.
# A mid pad of unique entries between the compiled and the runtime inputs holds them in place. name -> (input
# section, flags, entsize); the marker is the last entry (a symbol past the end of a merge section is not mapped).
# The flags must equal the compiled inputs' for the linker to merge, so these sections cannot carry SHF_GNU_RETAIN;
# a PC-relative word to each marker in the retained .rodata.lpad keeps them through --gc-sections.
MERGES = {
    "rodata.cst4": (".rodata.cst4", '"aM",@progbits,4', 4),
    "rodata.cst8": (".rodata.cst8", '"aM",@progbits,8', 8),
    "rodata.cst16": (".rodata.cst16", '"aM",@progbits,16', 16),
    "rodata.cst32": (".rodata.cst32", '"aM",@progbits,32', 32),
    "rodata.str1.1": (".rodata.str1.1", '"aMS",@progbits,1', 1),
}
EH = ".eh_frame"  # mid pad only: one FDE of 24 + extra bytes, extra a multiple of 8, made of DW_CFA_GNU_args_size
# ops (the linker strips trailing DW_CFA_nop padding, so nops would not count)
EH_FN = "__lpad_eh_fn"
EH_MIN = 24


def run(*cmd):
    return subprocess.run(cmd, check=True, capture_output=True, text=True).stdout


def sections(path):
    """Allocated sections in address order: [(name, addr, size, file offset)]."""
    secs = []
    for line in run("readelf", "-SW", path).splitlines():
        line = line.strip()
        if not line.startswith("[") or "]" not in line:
            continue
        f = line.split("]", 1)[1].split()
        if len(f) < 7:
            continue
        try:
            name, addr, off, size, flags = f[0], int(f[2], 16), int(f[3], 16), int(f[4], 16), f[6]
        except ValueError:
            continue
        if "A" in flags and addr:
            secs.append((name, addr, size, off))
    return sorted(secs, key=lambda s: (s[1], s[0]))


def loads(path):
    return [(int(f[2], 16), int(f[5], 16), f[6]) for f in (l.split() for l in run("readelf", "-lW", path).splitlines())
            if f and f[0] == "LOAD"]


def notes(path):
    """The GNU property note: a pad object must not change the linked features (IBT/SHSTK, ISA)."""
    r = subprocess.run(["readelf", "-x", ".note.gnu.property", path], capture_output=True, text=True)
    # hex dump lines start with the section address, which a pad may move: compare the bytes only
    return [l.split()[1:5] for l in r.stdout.splitlines() if l.strip().startswith("0x")]


def symbols(path):
    """Defined symbols: {name: addr} (first definition wins)."""
    out = {}
    for line in run("nm", "--defined-only", path).splitlines():
        f = line.split(None, 2)
        if len(f) == 3 and f[1] not in "UuwWvV":
            out.setdefault(f[2], int(f[0], 16))
    return out


def eh_marker(path, secs, syms):
    """End address of the pad FDE in .eh_frame: look its start up in the .eh_frame_hdr search table."""
    by = {s[0]: s for s in secs}
    if EH_FN not in syms or ".eh_frame_hdr" not in by or EH not in by:
        return None, 0
    hn, ha, hs, ho = by[".eh_frame_hdr"]
    en, ea, es, eo = by[EH]
    with open(path, "rb") as fh:
        fh.seek(ho)
        hdr = fh.read(hs)
        ver, ptr_enc, cnt_enc, tab_enc = hdr[0], hdr[1], hdr[2], hdr[3]
        if ver != 1 or cnt_enc != 0x03 or tab_enc != 0x3b or ptr_enc != 0x1b:
            sys.exit(f"{path}: unexpected .eh_frame_hdr encoding {hdr[:4].hex()}")
        count = struct.unpack_from("<I", hdr, 8)[0]
        want = syms[EH_FN] - ha
        lo, hi, fde = 0, count, None
        while lo < hi:
            mid = (lo + hi) // 2
            pc, f = struct.unpack_from("<ii", hdr, 12 + 8 * mid)
            if pc == want:
                fde = ha + f
                break
            lo, hi = (mid + 1, hi) if pc < want else (lo, mid)
        if fde is None:
            sys.exit(f"{path}: pad FDE for {EH_FN} not in the .eh_frame_hdr table (linked without the mid pad?)")
        fh.seek(eo + (fde - ea))
        length = struct.unpack("<I", fh.read(4))[0]
    return fde + 4 + length, count


def marks(path):
    """{section: (marker address, section end address, section start)} for every padded section (end None for .eh_frame)."""
    secs, syms = sections(path), symbols(path)
    by = {s[0]: s for s in secs}
    out = {}
    for n, (_, _, _, marker, _) in PADDABLE.items():
        if n in by and marker in syms:
            out[n] = (syms[marker], by[n][1] + by[n][2], by[n][1])
    for n in MERGES:
        if "__lpad_mid_" + n.replace(".", "_") in syms:
            out[n] = (syms["__lpad_mid_" + n.replace(".", "_")], None, None)
    e, count = eh_marker(path, secs, syms)
    if e is not None:
        out[EH] = (e, None, by[EH][1])
    return secs, out, count


def write_obj(prefix, pad_dir, src):
    obj = os.path.join(pad_dir, f"{prefix}-{hashlib.sha256(src.encode()).hexdigest()[:16]}.o")
    if not os.path.exists(obj):
        os.makedirs(pad_dir, exist_ok=True)
        s = obj[:-2] + ".S"
        open(s, "w").write(src)
        subprocess.run(["cc", "-c", s, "-o", obj], check=True)
    return obj


def zero_text():
    return mid_source({})


def mid_source(want, fdes=0):
    """Assembler source of the mid pad: every paddable section gets its extra bytes and a marker after them."""
    src = ""
    eh = want.get(EH, 0)
    for n, (name, flags, fill, marker, _) in PADDABLE.items():
        extra = want.get(n, 0)
        src += f".section {name},{flags}\n"
        if n == ".rodata":
            src += "".join(f".long {m} - .\n" for m in (f"__lpad_mid_{k.replace('.', '_')}" for k in MERGES))
        if n == ".text":
            # filler FDEs first (the first one carries the extra bytes); the last one is the marker's
            for i in range(fdes + 1):
                last = i == fdes
                src += f"{EH_FN}:\n" if last else f"__lpad_eh_fn_{i}:\n"
                src += ".cfi_startproc\n"
                if eh and i == 0:
                    src += f".rept {eh // 2}\n.cfi_escape 0x2e,0\n.endr\n"
                src += "ret\n.cfi_endproc\n"
        if extra:
            src += f".space {extra},{fill}\n" if fill else f".zero {extra}\n"
        src += f"{marker}:\n"
    for n, (name, flags, size) in MERGES.items():
        extra, label = want.get(n, 0), "__lpad_mid_" + n.replace(".", "_")
        # the linker merges inputs of one (flags, entsize, alignment) class: the alignment is the entry size
        src += f".section {name},{flags}\n.balign {size}\n"
        if size == 1:
            if extra:
                src += f".fill {extra - 1},1,0xfe\n.byte 0\n"
            src += f"{label}:\n.byte 0xfd,0xfc,0\n"
        else:
            tag = 0x4c500000 + 0x10000 * size
            for i in range(extra // size + 1):  # unique entries; the last one is the marker
                if i == extra // size:
                    src += f"{label}:\n"
                src += ".long " + ",".join(str(tag + i if k == 0 else 0x4c504144 + k) for k in range(size // 4)) + "\n"
    src += '.section .note.GNU-stack,"",@progbits\n'
    return src


def end_source(want):
    src = ""
    for n, (_, flags, fill, _, name) in PADDABLE.items():
        if want.get(n):
            src += f".section {name},{flags}\n" + (f".space {want[n]},{fill}\n" if fill else f".zero {want[n]}\n")
    return src + '.section .note.GNU-stack,"",@progbits\n' if src else ""


def write_arm(pads, arm, pad_dir):
    mid = write_obj(f"{arm}-mid", pad_dir, mid_source({k: v for k, v in pads[arm]["mid"].items() if v > 0}, pads[arm].get("fde", 0)))
    esrc = end_source({k: v for k, v in pads[arm]["end"].items() if v > 0})
    return mid, (write_obj(f"{arm}-end", pad_dir, esrc) if esrc else "-")


# .data.rel.ro lies in the RELRO region, which the linker anchors at its END (page aligned): a pad in it moves
# the section's start, not its end. So the runtime bytes before the compiled ones need the sections' sizes equal
# (mid pad) and the bytes after them need the markers equal (end pad, which lowers the marker).
ANCHORED = ".data.rel.ro"


def level(pads, kind, n, am, ah):
    r = ah - am
    if r == 0:
        return False
    gran = 8 if n == EH else MERGES[n][2] if n in MERGES else 1
    if r % gran:
        sys.exit(f"{n}: runtime start differs by {r} bytes: not a multiple of the {gran}-byte entry size")
    big, small = ("head", "main") if r > 0 else ("main", "head")
    r = abs(r)
    # keep pads minimal: first take back the longer arm's own pad, then pad the shorter arm
    back = min(r, pads[big][kind].get(n, 0))
    pads[big][kind][n] = pads[big][kind].get(n, 0) - back
    pads[small][kind][n] = pads[small][kind].get(n, 0) + r - back
    return True


def anchored(pads, mk_m, mk_h, size_m, size_h):
    n = ANCHORED
    old = {a: (pads[a]["end"].get(n, 0), pads[a]["mid"].get(n, 0)) for a in pads}
    # marker = anchor - (runtime bytes after it + end pad): the arm with the higher marker gets the end pad
    d = old["head"][0] - old["main"][0] + (mk_h - mk_m)
    end = {"head": max(d, 0), "main": max(-d, 0)}
    # start = anchor - size: the arm with the smaller section gets the mid pad
    diff = (size_h + end["head"] - old["head"][0]) - (size_m + end["main"] - old["main"][0])
    d = old["main"][1] - old["head"][1] + diff
    mid = {"main": max(d, 0), "head": max(-d, 0)}
    changed = False
    for a in pads:
        changed |= (end[a], mid[a]) != old[a]
        pads[a]["end"][n], pads[a]["mid"][n] = end[a], mid[a]
    return changed


def plan(main_bin, head_bin, pads_json, pad_dir):
    empty = lambda: {"mid": {}, "end": {}}
    pads = json.load(open(pads_json)) if os.path.exists(pads_json) else {"main": empty(), "head": empty()}
    secs_m, mk_m, cnt_m = marks(main_bin)
    secs_h, mk_h, cnt_h = marks(head_bin)
    names_m, names_h = [s[0] for s in secs_m], [s[0] for s in secs_h]
    if names_m != names_h:
        sys.exit(f"section order differs:\n main {names_m}\n head {names_h}")
    missing = [n for n in list(PADDABLE) + [EH] if n in names_m and n not in mk_m or n in names_h and n not in mk_h]
    missing += [n for n in MERGES if (n in mk_m) != (n in mk_h)]
    if missing:
        sys.exit(f"no mid-pad marker for {missing}: link both arms with the mid pad first (linkpad.py zero) and keep symbols")
    changed = False
    # The unwinder binary-searches the .eh_frame_hdr table: both arms need the same number of FDEs, so the
    # arm with fewer gets filler FDEs (each also adds 24 bytes of .eh_frame, which the section's pad absorbs).
    fde = {a: pads[a].setdefault("fde", 0) for a in ("main", "head")}
    d = fde["head"] - fde["main"] - (cnt_h - cnt_m)
    new = {"head": max(d, 0), "main": max(-d, 0)}
    if new != fde:
        pads["main"]["fde"], pads["head"]["fde"] = new["main"], new["head"]
        changed = True
    # Walk the sections in address order and fix the first one that differs; the sections after it are
    # re-read from the relinked binaries in the next round (a pad moves everything after it, and alignment
    # absorbs part of that, so modelling it makes the plan oscillate).
    for n in sorted(mk_m, key=lambda k: mk_m[k][0]) if not changed else ():
        (mm, em, sm), (mh, eh, sh) = mk_m[n], mk_h[n]
        if n == ANCHORED:
            changed = anchored(pads, mm, mh, em - sm if em else 0, eh - sh if eh else 0)
        else:
            changed = level(pads, "mid", n, mm, mh)
            if not changed and em is not None:
                changed = level(pads, "end", n, em, eh)
        if changed:
            break
    json.dump(pads, open(pads_json, "w"), indent=1, sort_keys=True)
    if not changed:
        print("CONVERGED")
        return
    (mm, em), (mh, eh) = write_arm(pads, "main", pad_dir), write_arm(pads, "head", pad_dir)
    print(f"PADS main={mm},{em} head={mh},{eh}")


def check(main_bin, head_bin, rt_lib=None):
    ok = True
    secs_m, mk_m, cnt_m = marks(main_bin)
    secs_h, mk_h, cnt_h = marks(head_bin)
    names_m, names_h = [s[0] for s in secs_m], [s[0] for s in secs_h]
    if names_m != names_h:
        print(f"section order differs:\n main {names_m}\n head {names_h}")
        print("LAYOUT-DIFFERS")
        return 1
    print(f"{'section':<20}{'marker main':>13}{'marker head':>13}{'end main':>12}{'end head':>12}")
    for n in sorted(mk_m, key=lambda k: mk_m[k][0]):
        (mm, em, sm), (mh, eh, sh) = mk_m[n], mk_h[n]
        # the RELRO region is anchored at its end: there the section start must coincide, not the end
        bad = mm != mh or (n == ANCHORED and sm != sh) or (n != ANCHORED and em is not None and em != eh)
        ok &= not bad
        print(f"{n:<20}{mm:>13x}{mh:>13x}{(em or 0):>12x}{(eh or 0):>12x}" + ("  DIFFERS" if bad else ""))
    if cnt_m != cnt_h:
        print(f"FDE count differs: main {cnt_m} head {cnt_h}")
        ok = False
    lm, lh = loads(main_bin), loads(head_bin)
    for (vm, mm_, fm), (vh, mh_, fh) in zip(lm, lh):
        if vm != vh or fm != fh:
            print(f"LOAD {fm} {vm:x} vs {fh} {vh:x}  START DIFFERS")
            ok = False
    em_, eh_ = lm[-1][0] + lm[-1][1], lh[-1][0] + lh[-1][1]
    print(f"image end (heap start side): main {em_:x} head {eh_:x}" + ("" if em_ == eh_ else "  DIFFERS"))
    if len(lm) != len(lh) or em_ != eh_:
        ok = False
    if notes(main_bin) != notes(head_bin):
        print(".note.gnu.property DIFFERS")
        ok = False
    # runtime symbols: defined in both binaries (and in the runtime archive), same address?
    sm, sh = symbols(main_bin), symbols(head_bin)
    common = set(sm) & set(sh)
    if rt_lib:
        common &= {l.split(None, 2)[2] for l in run("nm", "--defined-only", rt_lib).splitlines() if len(l.split(None, 2)) == 3}
    ranges = [(s[0], s[1], s[1] + s[2]) for s in secs_m]

    def sec_of(a):
        return next((r[0] for r in ranges if r[1] <= a < r[2]), "?")

    per = {}
    for name in common:
        sec = sec_of(sm[name])
        t = per.setdefault(sec, [0, 0, []])
        t[0] += 1
        if sm[name] != sh[name]:
            t[1] += 1
            t[2].append(name)
    print(f"runtime symbols in both binaries{' and the runtime archive' if rt_lib else ''}: same address / compared, per section")
    for sec, (cnt, moved, ex) in sorted(per.items(), key=lambda kv: -kv[1][0]):
        print(f"  {sec:<22}{cnt - moved:>8}/{cnt:<8}" + (f" moved e.g. {ex[0][:60]}" if moved else ""))
    print("LAYOUT-EQUAL" if ok else "LAYOUT-DIFFERS")
    return 0 if ok else 1


if __name__ == "__main__":
    cmd = sys.argv[1] if len(sys.argv) > 1 else ""
    if cmd == "zero" and len(sys.argv) == 3:
        print(write_obj("zero-mid", sys.argv[2], zero_text()))
    elif cmd == "plan" and len(sys.argv) == 6:
        plan(*sys.argv[2:])
    elif cmd == "check" and len(sys.argv) in (4, 5):
        sys.exit(check(*sys.argv[2:]))
    else:
        sys.exit(__doc__)
