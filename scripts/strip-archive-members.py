#!/usr/bin/env python3
"""Strip embedded LLVM bitcode (.llvmbc/.llvmcmd) from the members of static archives.

The UI and wasm-host archives are staticlibs built from rlib+staticlib crates,
and every archive bundles the Rust standard library from rustup's precompiled
rlibs. Those objects carry their LLVM bitcode in .llvmbc/.llvmcmd, which a
native link never reads. `llvm-objcopy` refuses a whole Windows .lib (import
members), so every platform takes the same path: split the archive into
members, run llvm-objcopy on each object member that has the sections, write
the archive back with the members in their original order and names, and let
llvm-ar rebuild the symbol table. No other byte of any object changes.

Usage: strip-archive-members.py [--strip-debug] ARCHIVE...
  --strip-debug  also drop debug info (.debug*, e.g. CodeView in a vendored
                 OpenSSL built by MSVC) from the same members.
Archives are rewritten in place. Env LLVM_BIN picks the llvm tools directory.
"""
import glob
import os
import shutil
import subprocess
import sys
import tempfile

AR_MAGIC = b"!<arch>\n"
IMPORT_SIG = b"\x00\x00\xff\xff"  # COFF short-import member header (not an object)


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
    """Return [(name, date, uid, gid, mode, payload)] for the real members."""
    if data[:8] != AR_MAGIC:
        sys.exit("not an ar archive")
    pos, longnames, members = 8, b"", []
    while pos < len(data):
        hdr = data[pos:pos + 60]
        if len(hdr) < 60 or hdr[58:60] != b"`\n":
            sys.exit(f"bad member header at {pos}")
        raw = hdr[:16].decode("latin1")
        date, uid, gid, mode = (hdr[16:28], hdr[28:34], hdr[34:40], hdr[40:48])
        size = int(hdr[48:58])
        payload = data[pos + 60:pos + 60 + size]
        pos += 60 + size + (size & 1)
        if raw.startswith("#1/") or raw.startswith("__.SYMDEF"):
            sys.exit("BSD/Mach-O archive layout is not supported")
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
    return members


def member_header(name_field, date, uid, gid, mode, size):
    return (name_field.ljust(16).encode("latin1") + date + uid + gid + mode
            + str(size).ljust(10).encode() + b"`\n")


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


def process(path, strip_debug):
    objcopy, llvm_ar = find_tool("llvm-objcopy"), find_tool("llvm-ar")
    args = ["--remove-section=.llvmbc", "--remove-section=.llvmcmd"]
    needles = [b".llvmbc", b".llvmcmd"]
    if strip_debug:
        args.append("--strip-debug")
        needles.append(b".debug")
    with open(path, "rb") as f:
        members = parse(f.read())
    before = os.path.getsize(path)
    changed = 0
    with tempfile.TemporaryDirectory() as tmp:
        out = []
        for name, date, uid, gid, mode, payload in members:
            if any(n in payload for n in needles):
                new = strip_member(objcopy, payload, tmp, args)
                changed += new != payload
                payload = new
            out.append((name, date, uid, gid, mode, payload))
        new_path = os.path.join(tmp, "new.a")
        with open(new_path, "wb") as f:
            f.write(build(out))
        subprocess.run([llvm_ar, "s", new_path], check=True)  # symbol table
        if len(parse(open(new_path, "rb").read())) != len(members):
            sys.exit("member count changed")
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
