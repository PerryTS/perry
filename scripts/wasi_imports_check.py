#!/usr/bin/env python3
"""Check which modules a WASI core module or runtime archive imports.

A wasm32-wasip1 build must depend on nothing but `wasi_snapshot_preview1`:
no `wasi:*` component interfaces and no host extensions. This reads the import
section of a core module, or of every wasm object in a static archive, and
fails on any import module outside the allowed set.

    wasi_imports_check.py libperry_runtime.a
    wasi_imports_check.py program.wasm
    wasi_imports_check.py --self-test

A relocatable object imports every symbol it leaves undefined from `env`; the
linker resolves those against the other archive members and libc. So in an
archive `env` is tolerated, while a linked module (where nothing is left to
resolve) may import only the allowed modules.

Liveness: the check fails when it finds no import from an allowed module, so
an empty or wrong input cannot pass.
"""

import argparse
import sys
import tempfile
from pathlib import Path

WASM_MAGIC = b"\0asm\x01\0\0\0"
ARCHIVE_MAGIC = b"!<arch>\n"
PREVIEW1 = "wasi_snapshot_preview1"


class Reader:
    def __init__(self, data: bytes, pos: int = 0):
        self.data, self.pos = data, pos

    def byte(self) -> int:
        value = self.data[self.pos]
        self.pos += 1
        return value

    def leb(self) -> int:
        result = shift = 0
        while True:
            b = self.byte()
            result |= (b & 0x7F) << shift
            if not b & 0x80:
                return result
            shift += 7

    def name(self) -> str:
        length = self.leb()
        text = self.data[self.pos:self.pos + length].decode("utf-8")
        self.pos += length
        return text

    def limits(self) -> None:
        flags = self.byte()
        self.leb()
        if flags & 1:
            self.leb()


def module_imports(data: bytes) -> list[tuple[str, str]]:
    """The (module, name) of every import of one wasm binary."""
    if data[:8] != WASM_MAGIC:
        raise ValueError("not a core wasm binary (a component, or not wasm at all)")
    imports = []
    pos = 8
    while pos < len(data):
        reader = Reader(data, pos)
        section = reader.byte()
        size = reader.leb()
        end = reader.pos + size
        if section == 2:
            for _ in range(reader.leb()):
                module, name = reader.name(), reader.name()
                kind = reader.byte()
                if kind == 0:  # function: type index
                    reader.leb()
                elif kind == 1:  # table: reference type, limits
                    reader.byte()
                    reader.limits()
                elif kind == 2:  # memory
                    reader.limits()
                elif kind == 3:  # global: value type, mutability
                    reader.byte()
                    reader.byte()
                elif kind == 4:  # tag: attribute, type index
                    reader.byte()
                    reader.leb()
                else:
                    raise ValueError(f"unknown import kind {kind}")
                imports.append((module, name))
        pos = end
    return imports


def archive_members(data: bytes):
    """The data of every wasm object in a static archive."""
    pos = len(ARCHIVE_MAGIC)
    while pos + 60 <= len(data):
        size = int(data[pos + 48:pos + 58])
        body = data[pos + 60:pos + 60 + size]
        pos += 60 + size + (size & 1)
        if body[:4] == b"\0asm":
            yield body


def collect(path: Path) -> tuple[list[tuple[str, str]], bool, int]:
    data = path.read_bytes()
    if data[:8] == ARCHIVE_MAGIC:
        imports, members = [], 0
        for member in archive_members(data):
            imports += module_imports(member)
            members += 1
        return imports, True, members
    return module_imports(data), False, 1


def check(path: Path, allowed: set[str]) -> int:
    imports, is_archive, members = collect(path)
    tolerated = allowed | ({"env"} if is_archive else set())
    modules: dict[str, int] = {}
    for module, _ in imports:
        modules[module] = modules.get(module, 0) + 1
    unexpected = sorted(m for m in modules if m not in tolerated)
    kind = f"archive ({members} wasm members)" if is_archive else "module"
    print(f"{path}: {kind}, imports by module: {dict(sorted(modules.items()))}")
    if unexpected:
        for module in unexpected:
            names = sorted({n for m, n in imports if m == module})[:5]
            print(f"::error::imports from `{module}`, allowed {sorted(allowed)}: {names}")
        return 1
    if not any(m in allowed for m in modules):
        print(f"::error::no import from {sorted(allowed)}; the input is empty or not a WASI build")
        return 1
    print(f"OK: only {sorted(allowed)} imported")
    return 0


def self_test() -> int:
    def leb(n: int) -> bytes:
        out = bytearray()
        while True:
            b, n = n & 0x7F, n >> 7
            out.append(b | (0x80 if n else 0))
            if not n:
                return bytes(out)

    def name(s: str) -> bytes:
        return leb(len(s.encode())) + s.encode()

    def wasm(*imports: tuple[str, str]) -> bytes:
        body = leb(len(imports)) + b"".join(name(m) + name(n) + b"\0" + leb(0) for m, n in imports)
        types = b"\x01\x60\x00\x00"
        return (WASM_MAGIC + b"\x01" + leb(len(types)) + types
                + b"\x02" + leb(len(body)) + body)

    def archive(*members: bytes) -> bytes:
        out = ARCHIVE_MAGIC
        for index, body in enumerate(members):
            header = f"m{index}.o/".ljust(16) + "0".ljust(12) + "0".ljust(6) + "0".ljust(6)
            header += "644".ljust(8) + str(len(body)).ljust(10) + "`\n"
            out += header.encode() + body + (b"\n" if len(body) & 1 else b"")
        return out

    assert module_imports(wasm(("a", "b"), ("c", "d"))) == [("a", "b"), ("c", "d")]
    allowed = {PREVIEW1}
    good = wasm((PREVIEW1, "fd_write"))
    cases = [
        ("preview1-only module passes", good, 0),
        ("host extension module fails", wasm((PREVIEW1, "fd_write"), ("perry_net", "resolve")), 1),
        ("component-model interface fails", wasm(("wasi:cli/stdout@0.2.0", "get-stdout")), 1),
        ("env import in a linked module fails", wasm((PREVIEW1, "fd_write"), ("env", "f")), 1),
        ("module without imports fails (liveness)", wasm(), 1),
        ("archive tolerates env", archive(good, wasm(("env", "memcpy"))), 0),
        ("archive with a host extension fails", archive(good, wasm(("perry_host", "discard_pages"))), 1),
        ("archive without wasm members fails", archive(b"not wasm"), 1),
    ]
    with tempfile.TemporaryDirectory() as directory:
        for title, data, expected in cases:
            path = Path(directory) / "input"
            path.write_bytes(data)
            status = check(path, allowed)
            assert status == expected, f"{title}: exit {status}, expected {expected}"
    print(f"self-test OK ({len(cases)} cases)")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("input", nargs="?", type=Path, help="a core wasm module or a static archive")
    parser.add_argument("--allow", action="append", metavar="MODULE",
                        help=f"an import module that may appear (default: {PREVIEW1})")
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        return self_test()
    if args.input is None:
        parser.error("an input file is required")
    return check(args.input, set(args.allow or [PREVIEW1]))


if __name__ == "__main__":
    sys.exit(main())
