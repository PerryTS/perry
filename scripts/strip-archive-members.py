#!/usr/bin/env python3
"""Strip embedded LLVM bitcode from the members of static archives.

The UI and wasm-host archives are staticlibs built from rlib+staticlib crates,
and every archive bundles the Rust standard library from rustup's precompiled
rlibs. Those objects carry their LLVM bitcode in .llvmbc/.llvmcmd, which a
native link never reads. `llvm-objcopy` refuses a whole Windows .lib (import
members), so every platform takes the same path: split the archive into
members, run llvm-objcopy on each object member that has the sections, write
the archive back with the members in their original order and names, and let
llvm-ar rebuild the symbol table. No other byte of any object changes.

ELF and COFF objects carry the bitcode in .llvmbc/.llvmcmd. Mach-O objects
carry it in __LLVM,__bitcode and __LLVM,__cmdline, and their archives use the
BSD layout (`#1/<len>` names stored in front of the member data, a
`__.SYMDEF` symbol table). llvm-objcopy rewrites a Mach-O object (offsets
move, the local labels of the removed sections go), so every rewritten Mach-O
member is checked against the original: each other section's bytes,
addresses and relocations, every other symbol and every other load command
must be unchanged, or the script fails. Members that are not rewritten stay
byte-identical, and an archive with nothing to strip is not rewritten.

Usage: strip-archive-members.py [--strip-debug] ARCHIVE...
  --strip-debug  also drop debug info (.debug*, e.g. CodeView in a vendored
                 OpenSSL built by MSVC) from the same members.
Archives are rewritten in place. Env LLVM_BIN picks the llvm tools directory.
"""
import glob
import os
import shutil
import struct
import subprocess
import sys
import tempfile

AR_MAGIC = b"!<arch>\n"
IMPORT_SIG = b"\x00\x00\xff\xff"  # COFF short-import member header (not an object)
BSD_SYMTABS = ("__.SYMDEF", "__.SYMDEF SORTED", "__.SYMDEF_64", "__.SYMDEF_64 SORTED")
MACHO_ARGS = ["--remove-section=__LLVM,__bitcode", "--remove-section=__LLVM,__cmdline"]


def find_tool(name):
    dirs = [os.environ["LLVM_BIN"]] if os.environ.get("LLVM_BIN") else []
    found = shutil.which(name)
    if found:
        return found
    for d in dirs + sorted(glob.glob("/usr/lib/llvm-*/bin"), reverse=True):
        p = os.path.join(d, name)
        if os.path.exists(p):
            return p
    sys.exit(f"strip-archive-members: {name} not found (install llvm, or set LLVM_BIN)")


def parse(data):
    """Return (layout, [(name, date, uid, gid, mode, payload)]) for the real
    members; layout is "bsd" (Darwin) or "gnu" (GNU and COFF)."""
    if data[:8] != AR_MAGIC:
        sys.exit("not an ar archive")
    pos, longnames, members, layout = 8, b"", [], None
    while pos < len(data):
        hdr = data[pos:pos + 60]
        if len(hdr) < 60 or hdr[58:60] != b"`\n":
            sys.exit(f"bad member header at {pos}")
        raw = hdr[:16].decode("latin1")
        date, uid, gid, mode = (hdr[16:28], hdr[28:34], hdr[34:40], hdr[40:48])
        size = int(hdr[48:58])
        payload = data[pos + 60:pos + 60 + size]
        pos += 60 + size + (size & 1)
        bsd = raw.startswith("#1/") or raw.startswith("__.SYMDEF")
        if layout is None:
            layout = "bsd" if bsd else "gnu"
        if layout == "bsd":
            name = raw.rstrip()
            if name.startswith("#1/"):
                n = int(name[3:])
                name, payload = payload[:n].rstrip(b"\0").decode("latin1"), payload[n:]
            if name not in BSD_SYMTABS:  # symbol table: rebuilt by llvm-ar
                members.append((name, date, uid, gid, mode, payload))
            continue
        if bsd:
            sys.exit(f"BSD member header in a GNU archive at {pos}")
        if raw.rstrip() == "/":
            continue  # symbol table (first/second linker member): rebuilt by llvm-ar
        if raw.rstrip() == "//":
            longnames = payload
            continue
        if raw[0] == "/" and raw[1:].strip().isdigit():
            off = int(raw[1:])
            # GNU tables end entries with "/\n", MS (COFF) tables with NUL.
            ends = [i for i in (longnames.find(b"\n", off), longnames.find(b"\0", off)) if i >= 0]
            end = min(ends) if ends else len(longnames)
            name = longnames[off:end].decode("latin1")
            if name.endswith("/"):
                name = name[:-1]
        else:
            name = raw.rstrip()
            if name.endswith("/"):
                name = name[:-1]
        members.append((name, date, uid, gid, mode, payload))
    return layout, members


def member_header(name_field, date, uid, gid, mode, size):
    return (name_field.ljust(16).encode("latin1") + date + uid + gid + mode
            + str(size).ljust(10).encode() + b"`\n")


def build_bsd(members):
    """Darwin layout: every name as `#1/<len>` in front of the data, padded
    with NULs so the object starts 8-aligned (as Rust's and LLVM's writers
    do). llvm-ar then adds the __.SYMDEF member."""
    out, pos = [AR_MAGIC], len(AR_MAGIC)
    for name, date, uid, gid, mode, payload in members:
        n = name.encode("latin1")
        n += b"\0" * (-(pos + 60 + len(n)) % 8)
        body = n + payload
        out.append(member_header("#1/%d" % len(n), date, uid, gid, mode, len(body)))
        out.append(body)
        pos += 60 + len(body)
        if len(body) & 1:
            out.append(b"\n")
            pos += 1
    return b"".join(out)


def build(members):
    long_tab, entries = b"", []
    for name, date, uid, gid, mode, payload in members:
        n = name.encode("latin1") + b"/"
        if len(n) <= 16:
            field = n.decode("latin1")
        else:
            field = "/%d" % len(long_tab)
            long_tab += n + b"\n"
        entries.append((field, date, uid, gid, mode, payload))
    out = [AR_MAGIC]

    def add(field, date, uid, gid, mode, payload):
        out.append(member_header(field, date, uid, gid, mode, len(payload)))
        out.append(payload)
        if len(payload) & 1:
            out.append(b"\n")

    if long_tab:
        add("//", b"0".ljust(12), b"0".ljust(6), b"0".ljust(6), b"0".ljust(8), long_tab)
    for e in entries:
        add(*e)
    return b"".join(out)


MH_MAGIC_64 = b"\xcf\xfa\xed\xfe"
CPU_ARM64 = 0x0100000C
ARM64_RELOC_ADDEND = 10  # r_symbolnum is an addend, not a symbol or section
LC_SEGMENT_64, LC_SYMTAB, LC_DYSYMTAB = 0x19, 0x2, 0xB
LINKEDIT_DATA = {0x1D, 0x1E, 0x26, 0x29, 0x2B, 0x2E, 0x80000033, 0x80000034}
ZEROFILL = {0x1, 0x0C, 0x12}


def macho(o):
    """Parse a 64-bit Mach-O object: (header, sections, symbols, other load
    commands, dysymtab counts). Sections are dicts; symbols are
    (name, n_type, n_sect, n_desc, n_value)."""
    cpu, _sub, filetype, ncmds, _size, flags = struct.unpack_from("<iiIIII", o, 4)
    pos, secs, syms, other, dysym = 32, [], [], [], None
    for _ in range(ncmds):
        cmd, size = struct.unpack_from("<II", o, pos)
        if cmd == LC_SEGMENT_64:
            for i in range(struct.unpack_from("<I", o, pos + 64)[0]):
                h = pos + 72 + 80 * i
                (addr, sz, off, align, reloff, nreloc, fl, r1, r2) = struct.unpack_from("<QQIIIIIII", o, h + 32)
                secs.append({
                    "name": (o[h + 16:h + 32].rstrip(b"\0"), o[h:h + 16].rstrip(b"\0")),
                    "head": (addr, sz, align, fl, r1, r2),
                    "data": b"" if fl & 0xFF in ZEROFILL else o[off:off + sz],
                    "relocs": [struct.unpack_from("<iI", o, reloff + 8 * k) for k in range(nreloc)],
                })
        elif cmd == LC_SYMTAB:
            symoff, nsyms, stroff, _ = struct.unpack_from("<IIII", o, pos + 8)
            for k in range(nsyms):
                strx, ty, sect, desc, val = struct.unpack_from("<IBBHQ", o, symoff + 16 * k)
                name = o[stroff + strx:o.index(b"\0", stroff + strx)]
                syms.append((name, ty, sect, desc, val))
        elif cmd == LC_DYSYMTAB:
            f = struct.unpack_from("<18I", o, pos + 8)
            # nextdefsym, nundefsym, ntoc, nmodtab, nextrefsyms, nextrel, nlocrel
            dysym = (f[3], f[5], f[7], f[9], f[11], f[15], f[17]), [
                struct.unpack_from("<I", o, f[12] + 4 * k)[0] for k in range(f[13])]
        elif cmd in LINKEDIT_DATA:
            off, sz = struct.unpack_from("<II", o, pos + 8)
            other.append((cmd, o[off:off + sz]))
        else:
            other.append((cmd, o[pos + 8:pos + size]))
        pos += size
    return (cpu, filetype, flags), secs, syms, other, dysym


def has_llvm_section(payload):
    return payload[:4] == MH_MAGIC_64 and any(
        sec["name"][0] == b"__LLVM" for sec in macho(payload)[1])


def macho_view(o):
    """Everything a link reads, outside the __LLVM segment, with section and
    symbol indices replaced by names (removing sections renumbers both)."""
    head, secs, syms, other, dysym = macho(o)
    names = [sec["name"] for sec in secs]

    def sect(n):
        return names[n - 1] if 1 <= n <= len(names) else n

    def symname(n):
        return syms[n][0] if n < len(syms) else n  # INDIRECT_SYMBOL_LOCAL/ABS

    def reloc(addr, info):
        num, ext, ty = info & 0xFFFFFF, (info >> 27) & 1, info >> 28
        if ext:
            target = ("sym", syms[num][0])
        elif head[0] == CPU_ARM64 and ty == ARM64_RELOC_ADDEND:
            target = ("addend", num)
        else:
            target = ("sect", sect(num))
        return addr, info & 0xFF000000, target

    kept = [(n, ty, sect(sc) if sc else 0, d, v) for n, ty, sc, d, v in syms
            if not (1 <= sc <= len(names) and names[sc - 1][0] == b"__LLVM")]
    view_secs = [(sec["name"], sec["head"], sec["data"], [reloc(*r) for r in sec["relocs"]])
                 for sec in secs if sec["name"][0] != b"__LLVM"]
    view_dysym = dysym and (dysym[0], [symname(n) for n in dysym[1]])
    return head, view_secs, kept, other, view_dysym


def strip_member(objcopy, payload, tmp, args):
    src, dst = os.path.join(tmp, "in.o"), os.path.join(tmp, "out.o")
    with open(src, "wb") as f:
        f.write(payload)
    r = subprocess.run([objcopy, *args, src, dst], capture_output=True, text=True)
    if r.returncode != 0:
        if payload[:4] == IMPORT_SIG and payload[4:6] == b"\x00\x00":
            return payload  # import-library member: no sections to strip
        sys.exit(f"llvm-objcopy failed: {r.stderr.strip()}")
    with open(dst, "rb") as f:
        return f.read()


def same_member_payload(layout, expected, written):
    """llvm-ar may fill a BSD member to 8-byte alignment with newlines.

    Only the exact fill after all original bytes is archive padding; a
    changed content byte or any other suffix is still a payload change.
    """
    return written == expected or (
        layout == "bsd" and written == expected + b"\n" * (-len(expected) % 8)
    )


def process(path, strip_debug):
    objcopy, llvm_ar = find_tool("llvm-objcopy"), find_tool("llvm-ar")
    args = ["--remove-section=.llvmbc", "--remove-section=.llvmcmd"]
    needles = [b".llvmbc", b".llvmcmd"]
    if strip_debug:
        args.append("--strip-debug")
        needles.append(b".debug")
    with open(path, "rb") as f:
        layout, members = parse(f.read())
    if layout == "bsd" and strip_debug:
        sys.exit("--strip-debug is not supported for Mach-O archives")
    before = os.path.getsize(path)
    changed = 0
    with tempfile.TemporaryDirectory() as tmp:
        out = []
        for name, date, uid, gid, mode, payload in members:
            if layout == "bsd":
                if has_llvm_section(payload):
                    new = strip_member(objcopy, payload, tmp, MACHO_ARGS)
                    if has_llvm_section(new) or macho_view(new) != macho_view(payload):
                        sys.exit(f"{name}: llvm-objcopy changed more than the __LLVM sections")
                    changed += 1
                    payload = new
            elif any(n in payload for n in needles):
                new = strip_member(objcopy, payload, tmp, args)
                changed += new != payload
                payload = new
            out.append((name, date, uid, gid, mode, payload))
        if changed == 0:
            print(f"  {os.path.basename(path)}: nothing to strip ({len(members)} members)")
            return
        new_path = os.path.join(tmp, "new.a")
        with open(new_path, "wb") as f:
            f.write(build_bsd(out) if layout == "bsd" else build(out))
        subprocess.run([llvm_ar, "s", new_path], check=True)  # symbol table
        new_layout, written = parse(open(new_path, "rb").read())
        if new_layout != layout or [m[0] for m in written] != [m[0] for m in out]:
            sys.exit("member layout, names or order changed")
        if any(not same_member_payload(layout, m[5], w[5]) for w, m in zip(written, out)):
            sys.exit("member payload changed while writing the archive")
        shutil.copyfile(new_path, path)
    print(f"  stripped {os.path.basename(path)}: {changed}/{len(members)} members, "
          f"{before} -> {os.path.getsize(path)} bytes")


if __name__ == "__main__":
    argv = sys.argv[1:]
    strip_debug = "--strip-debug" in argv
    archives = [a for a in argv if a != "--strip-debug"]
    if not archives:
        sys.exit(__doc__)
    for a in archives:
        process(a, strip_debug)
