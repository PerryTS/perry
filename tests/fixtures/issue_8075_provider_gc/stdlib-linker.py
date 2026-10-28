#!/usr/bin/env python3
"""Link the stdlib against one runtime, including Rust dispatch and TLS state.

Cargo still compiles against an rlib, retaining the normal panic-abort profile.
At final link, a Rust dylib reexports that exact rlib and its dependency graph;
the stdlib imports its symbols instead of including a second runtime archive.
"""
import os
import json
from pathlib import Path
import re
import shlex
import subprocess
import sys


def defined_symbols(library, darwin):
    flags = ["-gU"] if darwin else ["-D", "--defined-only"]
    output = subprocess.check_output(["nm", *flags, str(library)], text=True)
    return {line.split()[-1] for line in output.splitlines() if len(line.split()) >= 3}


def build_runtime(arguments, archive, library, darwin, real_cc):
    source = library.parent / "runtime-provider.rs"
    source.write_text("pub extern crate perry_runtime;\n")
    metadata = Path(archive).with_suffix(".rmeta")
    externs = ["--extern", "perry_runtime=" + archive]
    # Current Cargo separates full metadata from the final-link rlib stub.
    # Use the adjacent file selected by that exact artifact identity.
    if metadata.is_file():
        externs.extend(["--extern", "perry_runtime=" + str(metadata)])
    driver = library.parent / "runtime-native-linker.sh"
    driver.write_text("#!/bin/sh\nexec " + shlex.quote(sys.executable) + " " + shlex.quote(__file__) +
                      " --runtime-native-link \"$@\"\n")
    driver.chmod(0o755)
    command = [
        os.environ.get("RUSTC", "rustc"), "--edition=2021",
        "--crate-name", "perry_provider_runtime", "--crate-type=dylib",
        "-C", "panic=abort", "-C", "opt-level=2", "-C", "codegen-units=16",
        "-C", "force-unwind-tables=yes", "-Z", "use-sync-unwind=yes",
        "-C", "linker=" + str(driver),
        *externs, str(source), "-o", str(library),
    ]
    # Cargo's artifact layout can put every dependency in a separate directory.
    # Resolve by the original crate metadata, never by a guessed archive glob.
    directories = dict.fromkeys(str(Path(a).parent) for a in arguments if a.endswith(".rlib"))
    # Derive crates do not occur in the final native linker arguments, but the
    # full runtime metadata can reference them. Let rustc match their recorded
    # crate identities in Cargo's adjacent artifact directories.
    artifact_root = next((p for p in Path(archive).parents if p.name == "build"), None)
    if artifact_root is not None:
        for directory in artifact_root.glob("*/*/out"):
            if any(directory.glob("*.so")) or any(directory.glob("*.dylib")) or any(directory.glob("*.dll")):
                directories[str(directory)] = None
    for directory in directories:
        command.extend(["-L", "dependency=" + directory])
    for index, argument in enumerate(arguments):
        if argument == "-L":
            command.extend(["-L", "native=" + arguments[index + 1]])
        elif argument.startswith("-L") and len(argument) > 2:
            command.extend(["-L", "native=" + argument[2:]])
    if darwin:
        command.extend([
            "-C", "link-arg=-Wl,-install_name,@rpath/" + library.name,
            "-C", "link-arg=-framework", "-C", "link-arg=CoreFoundation",
            "-C", "link-arg=-framework", "-C", "link-arg=Foundation",
        ])
    else:
        command.extend(["-C", "link-arg=-Wl,-soname," + library.name])
    (library.parent / "runtime-provider-command.json").write_text(json.dumps(command, indent=2) + "\n")
    result = subprocess.run(command)
    if result.returncode:
        raise SystemExit(result.returncode)


def runtime_native_link(arguments):
    # rustc's dylib export list covers Rust definitions, while these two
    # landing-pad functions come from bundled LLVM IR. Both images must use
    # the runtime's same trap transport, including its worker-base identity.
    native = ["perry_trap_call", "perry_trap_call_base"]
    rewritten = []
    for index, argument in enumerate(arguments):
        if argument.startswith("-Wl,--version-script="):
            original = Path(argument.split("=", 1)[1])
            text = original.read_text()
            text, count = re.subn(r"\bglobal\s*:", "global:\n" + "\n".join(name + ";" for name in native), text, count=1)
            if count != 1:
                raise SystemExit("expected rustc runtime global export scope")
            exports = original.with_name("perry-runtime-exports.map")
            exports.write_text(text)
            rewritten.append("-Wl,--version-script=" + str(exports))
        elif argument.startswith("-Wl,-exported_symbols_list,") or (index and arguments[index - 1] == "-Wl,-exported_symbols_list"):
            prefix = "-Wl,-exported_symbols_list," if argument.startswith("-Wl,-exported_symbols_list,") else "-Wl,"
            original = Path(argument.removeprefix(prefix))
            exports = original.with_name("perry-runtime-exports.list")
            exports.write_text(original.read_text() + "\n" + "\n".join("_" + name for name in native) + "\n")
            rewritten.append(prefix + str(exports))
        else:
            rewritten.append(argument)
    real_cc = os.environ.get("PERRY_ISSUE_8075_REAL_CC", "/usr/bin/cc")
    os.execv(real_cc, [real_cc, *rewritten])


def main():
    if sys.argv[1:2] == ["--runtime-native-link"]:
        runtime_native_link(sys.argv[2:])
    original = sys.argv[1:]
    real_cc = os.environ.get("PERRY_ISSUE_8075_REAL_CC", "/usr/bin/cc")
    runtimes = [a for a in original if Path(a).name.startswith("libperry_runtime-") and a.endswith(".rlib")]
    if not runtimes:
        os.execv(real_cc, [real_cc, *original])
    if len(runtimes) != 1:
        raise SystemExit("expected exactly one Cargo-selected runtime rlib")
    library = Path(os.environ["PERRY_ISSUE_8075_RUNTIME_LIBRARY"]).resolve()
    darwin = sys.platform == "darwin"
    build_runtime(original, runtimes[0], library, darwin, real_cc)
    provided = defined_symbols(library, darwin)
    arguments = []
    index = 0
    while index < len(original):
        argument = original[index]
        if argument == runtimes[0]:
            if darwin:
                arguments.append(str(library))
            else:
                arguments.extend(["-Wl,-Bdynamic", "-Wl,--no-as-needed", str(library),
                                  "-Wl,--as-needed", "-Wl,-Bstatic"])
        elif argument.startswith("-Wl,--version-script="):
            path = Path(argument.split("=", 1)[1])
            # rustc listed the runtime's C exports when it planned a static
            # link. Remove ONLY symbols proven provided by the runtime dylib;
            # retain --no-undefined-version for every actual stdlib export.
            lines = []
            for line in path.read_text().splitlines(keepends=True):
                match = re.fullmatch(r'\s*"?([^";\s]+)"?;\s*', line)
                if not match or match.group(1) not in provided:
                    lines.append(line)
            exports = library.parent / "stdlib-exports.map"
            exports.write_text("".join(lines))
            arguments.append("-Wl,--version-script=" + str(exports))
        elif argument == "-Wl,-exported_symbols_list" or argument.startswith("-Wl,-exported_symbols_list,"):
            if argument == "-Wl,-exported_symbols_list":
                index += 1
                path = Path(original[index].removeprefix("-Wl,"))
            else:
                path = Path(argument.removeprefix("-Wl,-exported_symbols_list,"))
            exports = library.parent / "stdlib-exports.list"
            exports.write_text("\n".join(sorted(set(path.read_text().splitlines()) - provided)) + "\n")
            arguments.extend(["-Wl,-exported_symbols_list", "-Wl," + str(exports)])
        else:
            arguments.append(argument)
        index += 1
    if darwin:
        arguments.extend(["-framework", "CoreFoundation", "-framework", "Foundation", "-Wl,-rpath,@loader_path"])
    else:
        arguments.extend(["-Wl,-rpath,$ORIGIN", "-Wl,-soname,libperry_stdlib.so"])
    os.execv(real_cc, [real_cc, *arguments])


if __name__ == "__main__":
    main()
