"""Check what a WASI core module or runtime archive imports.

A wasm32-wasip1 build must import nothing but functions of
`wasi_snapshot_preview1`, each with its standard signature. That rules out
`wasi:*` component interfaces, host extensions and any other host function, so
the module runs on a host that implements preview 1 and nothing else.

The opt-in `PERRY_WASM_NET=1` build also imports the three `perry_net`
functions. `--allow-perry-net` admits those, by name and signature, and
nothing else: a different `perry_net` function, a `perry_net` import with
another signature or any `perry_host` import still fails.

    wasi_imports_check.py libperry_runtime.a --allow-perry-net
    wasi_imports_check.py program.wasm
    wasi_imports_check.py program.wasm --allow-perry-net
    wasi_imports_check.py --self-test

The allowlist is per import, not per module: it names every permitted function
and its parameter and result types, read from the type section. A relocatable
object imports every symbol it leaves undefined from `env`; the linker
resolves those against the other archive members and libc. So in an archive
`env` is tolerated, while a linked module (where nothing is left to resolve)
may import only the allowed functions.

Liveness: the check fails when it finds no preview 1 import, so an empty or
wrong input cannot pass.
"""

import argparse
import sys
import tempfile
from pathlib import Path

WASM_MAGIC = b"\0asm\x01\0\0\0"
ARCHIVE_MAGIC = b"!<arch>\n"
PREVIEW1 = "wasi_snapshot_preview1"
PERRY_NET = "perry_net"

# Every function of wasi_snapshot_preview1 (the witx definition) with its core
# signature: `i` is i32, `l` is i64; before the dash the parameters, after it
# the results.
_PREVIEW1 = {
    "args_get": "ii-i", "args_sizes_get": "ii-i",
    "clock_res_get": "ii-i", "clock_time_get": "ili-i",
    "environ_get": "ii-i", "environ_sizes_get": "ii-i",
    "fd_advise": "illi-i", "fd_allocate": "ill-i", "fd_close": "i-i",
    "fd_datasync": "i-i", "fd_fdstat_get": "ii-i", "fd_fdstat_set_flags": "ii-i",
    "fd_fdstat_set_rights": "ill-i", "fd_filestat_get": "ii-i",
    "fd_filestat_set_size": "il-i", "fd_filestat_set_times": "illi-i",
    "fd_pread": "iiili-i", "fd_prestat_get": "ii-i", "fd_prestat_dir_name": "iii-i",
    "fd_pwrite": "iiili-i", "fd_read": "iiii-i", "fd_readdir": "iiili-i",
    "fd_renumber": "ii-i", "fd_seek": "ilii-i", "fd_sync": "i-i", "fd_tell": "ii-i",
    "fd_write": "iiii-i",
    "path_create_directory": "iii-i", "path_filestat_get": "iiiii-i",
    "path_filestat_set_times": "iiiilli-i",
    "path_link": "iiiiiii-i", "path_open": "iiiiillii-i",
    "path_readlink": "iiiiii-i", "path_remove_directory": "iii-i",
    "path_rename": "iiiiii-i", "path_symlink": "iiiii-i", "path_unlink_file": "iii-i",
    "poll_oneoff": "iiii-i", "proc_exit": "i-", "proc_raise": "i-i",
    "random_get": "ii-i", "sched_yield": "-i",
    "sock_accept": "iii-i", "sock_recv": "iiiiii-i", "sock_send": "iiiii-i",
    "sock_shutdown": "ii-i",
}
# `perry_net` (docs/WASI_P1_OUTPUT.md): tcp_connect(addr, len, fd_out),
# sock_error(fd, err_out), resolve(host, len, family, fd_out); each returns an
# errno.
_PERRY_NET = {"tcp_connect": "iii-i", "sock_error": "ii-i", "resolve": "iiii-i"}

_VALTYPES = {"i": 0x7F, "l": 0x7E}


def _signature(text: str) -> tuple[tuple[int, ...], tuple[int, ...]]:
    params, results = text.split("-")
    return (tuple(_VALTYPES[c] for c in params), tuple(_VALTYPES[c] for c in results))


ALLOWED = {
    PREVIEW1: {name: _signature(sig) for name, sig in _PREVIEW1.items()},
    PERRY_NET: {name: _signature(sig) for name, sig in _PERRY_NET.items()},
}


class Reader:
    def __init__(self, data: bytes, pos: int = 0):
        self.data, self.pos = data, pos

    def byte(self) -> int:
        if self.pos >= len(self.data):
            raise ValueError("truncated wasm module")
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
        if self.pos + length > len(self.data):
            raise ValueError("truncated wasm module")
        text = self.data[self.pos:self.pos + length].decode("utf-8")
        self.pos += length
        return text

    def limits(self) -> None:
        flags = self.byte()
        self.leb()
        if flags & 1:
            self.leb()


Signature = tuple[tuple[int, ...], tuple[int, ...]]
# (module, name, kind, signature); the signature is None unless the import is
# a function whose type the type section describes.
Import = tuple[str, str, int, "Signature | None"]


def module_imports(data: bytes) -> list[Import]:
    """Every import of one wasm binary, with the signature of each function."""
    if data[:8] != WASM_MAGIC:
        raise ValueError("not a core wasm binary (a component, or not wasm at all)")
    types: list[Signature] = []
    imports: list[Import] = []
    pos = 8
    while pos < len(data):
        reader = Reader(data, pos)
        section = reader.byte()
        size = reader.leb()
        end = reader.pos + size
        if end > len(data):
            raise ValueError("truncated wasm module")
        if section == 1:
            for _ in range(reader.leb()):
                if reader.byte() != 0x60:
                    raise ValueError("unsupported type form in the type section")
                params = tuple(reader.byte() for _ in range(reader.leb()))
                results = tuple(reader.byte() for _ in range(reader.leb()))
                types.append((params, results))
        elif section == 2:
            for _ in range(reader.leb()):
                module, name = reader.name(), reader.name()
                kind = reader.byte()
                signature = None
                if kind == 0:  # function: type index
                    index = reader.leb()
                    if index >= len(types):
                        raise ValueError(f"import {module}.{name} has no type {index}")
                    signature = types[index]
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
                imports.append((module, name, kind, signature))
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


def collect(path: Path) -> tuple[list[Import], bool, int]:
    data = path.read_bytes()
    if data[:8] == ARCHIVE_MAGIC:
        imports, members = [], 0
        for member in archive_members(data):
            imports += module_imports(member)
            members += 1
        return imports, True, members
    return module_imports(data), False, 1


def _text(signature: Signature) -> str:
    names = {0x7F: "i32", 0x7E: "i64", 0x7D: "f32", 0x7C: "f64"}
    return "(%s) -> (%s)" % tuple(
        ", ".join(names.get(t, hex(t)) for t in part) for part in signature)


def violations(imports: list[Import], is_archive: bool, allow_perry_net: bool) -> list[str]:
    """Why each import is not allowed; empty when all are."""
    allowed = {PREVIEW1: ALLOWED[PREVIEW1]}
    if allow_perry_net:
        allowed[PERRY_NET] = ALLOWED[PERRY_NET]
    found = []
    for module, name, kind, signature in imports:
        if module == "env" and is_archive:
            continue
        where = f"{module}.{name}"
        if module not in allowed:
            found.append(f"{where}: module `{module}` is not allowed "
                         f"(allowed: {sorted(allowed)})")
        elif name not in allowed[module]:
            found.append(f"{where}: not a function of `{module}`")
        elif kind != 0:
            found.append(f"{where}: imported as something other than a function")
        elif signature != allowed[module][name]:
            found.append(f"{where}: signature {_text(signature)}, "
                         f"expected {_text(allowed[module][name])}")
    return found


def check(path: Path, allow_perry_net: bool = False) -> int:
    imports, is_archive, members = collect(path)
    modules: dict[str, int] = {}
    for module, *_ in imports:
        modules[module] = modules.get(module, 0) + 1
    kind = f"archive ({members} wasm members)" if is_archive else "module"
    print(f"{path}: {kind}, imports by module: {dict(sorted(modules.items()))}")
    found = violations(imports, is_archive, allow_perry_net)
    for reason in found:
        print(f"::error::{reason}")
    if found:
        return 1
    if PREVIEW1 not in modules:
        print(f"::error::no import from {PREVIEW1}; the input is empty or not a WASI build")
        return 1
    extra = f" and {PERRY_NET}" if PERRY_NET in modules else ""
    print(f"OK: only {PREVIEW1}{extra} imported")
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

    def func(module: str, field: str, signature: str | None = None) -> tuple:
        """A function import; by default with its standard signature."""
        if signature is None:
            signature = {**_PREVIEW1, **_PERRY_NET}.get(field, "-")
        return (module, field, 0, signature)

    def wasm(*imports: tuple, component: bool = False) -> bytes:
        types, entries = [], []
        for module, field, kind, signature in imports:
            if kind == 0:
                params, results = signature.split("-")
                code = {"i": b"\x7f", "l": b"\x7e"}
                encoded = (b"\x60" + leb(len(params)) + b"".join(code[c] for c in params)
                           + leb(len(results)) + b"".join(code[c] for c in results))
                if encoded not in types:
                    types.append(encoded)
                entries.append(name(module) + name(field) + b"\0" + leb(types.index(encoded)))
            else:  # a memory import
                entries.append(name(module) + name(field) + b"\x02\0" + leb(1))
        type_body = leb(len(types)) + b"".join(types)
        body = leb(len(entries)) + b"".join(entries)
        magic = b"\0asm\x0d\0\x01\0" if component else WASM_MAGIC
        return (magic + b"\x01" + leb(len(type_body)) + type_body
                + b"\x02" + leb(len(body)) + body)

    def archive(*members: bytes) -> bytes:
        out = ARCHIVE_MAGIC
        for index, body in enumerate(members):
            header = f"m{index}.o/".ljust(16) + "0".ljust(12) + "0".ljust(6) + "0".ljust(6)
            header += "644".ljust(8) + str(len(body)).ljust(10) + "`\n"
            out += header.encode() + body + (b"\n" if len(body) & 1 else b"")
        return out

    parsed = module_imports(wasm(func("a", "b", "il-i"), func("c", "d", "-")))
    assert parsed == [("a", "b", 0, ((0x7F, 0x7E), (0x7F,))), ("c", "d", 0, ((), ()))], parsed
    for cut in (1, 3):
        try:
            module_imports(wasm(func(PREVIEW1, "fd_write"))[:-cut])
        except ValueError:
            pass
        else:
            raise AssertionError(f"a module truncated by {cut} bytes must be rejected")
    assert set(_PREVIEW1) == set(ALLOWED[PREVIEW1]) and len(_PREVIEW1) == 46

    fd_write = func(PREVIEW1, "fd_write")
    net = [func(PERRY_NET, n) for n in _PERRY_NET]
    # (title, binary, allow_perry_net, expected exit status)
    cases = [
        ("preview1-only module passes", wasm(fd_write), False, 0),
        ("every preview1 function with its signature passes",
         wasm(*(func(PREVIEW1, n) for n in _PREVIEW1)), False, 0),
        ("module with the three perry_net imports passes when allowed",
         wasm(fd_write, *net), True, 0),
        ("allowing perry_net does not require it", wasm(fd_write), True, 0),
        ("perry_net imports fail by default", wasm(fd_write, *net), False, 1),
        ("one perry_net import fails by default",
         wasm(fd_write, func(PERRY_NET, "resolve")), False, 1),
        ("perry_net import with another signature fails",
         wasm(fd_write, func(PERRY_NET, "resolve", "iii-i")), True, 1),
        ("perry_net import with a result type of i64 fails",
         wasm(fd_write, func(PERRY_NET, "sock_error", "ii-l")), True, 1),
        ("unknown perry_net function fails even when allowed",
         wasm(fd_write, func(PERRY_NET, "bind", "iii-i")), True, 1),
        ("perry_net imported as a memory fails",
         wasm(fd_write, (PERRY_NET, "resolve", 2, None)), True, 1),
        ("preview1 function with another signature fails",
         wasm(func(PREVIEW1, "fd_write", "iii-i")), False, 1),
        ("preview1 function with a 64-bit parameter where 32 is standard fails",
         wasm(func(PREVIEW1, "fd_close", "l-i")), False, 1),
        ("unknown preview1 function fails",
         wasm(fd_write, func(PREVIEW1, "sock_open", "iii-i")), False, 1),
        ("preview1 imported as a memory fails",
         wasm(fd_write, (PREVIEW1, "memory", 2, None)), False, 1),
        ("perry_host extension fails by default",
         wasm(fd_write, func("perry_host", "discard_pages", "ii-i")), False, 1),
        ("perry_host extension fails even when perry_net is allowed",
         wasm(fd_write, *net, func("perry_host", "discard_pages", "ii-i")), True, 1),
        ("component-model interface fails",
         wasm(func("wasi:cli/stdout@0.2.0", "get-stdout")), False, 1),
        ("a component binary fails", wasm(fd_write, component=True), False, 1),
        ("env import in a linked module fails", wasm(fd_write, func("env", "f")), False, 1),
        ("module without imports fails (liveness)", wasm(), False, 1),
        ("module of perry_net imports alone fails (liveness)", wasm(*net), True, 1),
        ("archive tolerates env", archive(wasm(fd_write), wasm(func("env", "memcpy"))), False, 0),
        ("archive with the perry_net member passes when allowed",
         archive(wasm(fd_write), wasm(*net)), True, 0),
        ("archive with the perry_net member fails by default",
         archive(wasm(fd_write), wasm(*net)), False, 1),
        ("archive with a host extension fails",
         archive(wasm(fd_write), wasm(func("perry_host", "discard_pages", "ii-i"))), True, 1),
        ("archive without wasm members fails", archive(b"not wasm"), False, 1),
    ]
    with tempfile.TemporaryDirectory() as directory:
        for title, data, allow_perry_net, expected in cases:
            path = Path(directory) / "input"
            path.write_bytes(data)
            try:
                status = check(path, allow_perry_net)
            except ValueError:
                status = 1
            assert status == expected, f"{title}: exit {status}, expected {expected}"
    print(f"self-test OK ({len(cases)} cases)")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("input", nargs="?", type=Path, help="a core wasm module or a static archive")
    parser.add_argument("--allow-perry-net", action="store_true",
                        help="also allow the three perry_net imports (PERRY_WASM_NET=1 builds)")
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        return self_test()
    if args.input is None:
        parser.error("an input file is required")
    return check(args.input, args.allow_perry_net)


if __name__ == "__main__":
    sys.exit(main())
