#!/usr/bin/env python3
"""Link the stdlib against one runtime, including Rust dispatch and TLS state.

Cargo still compiles against an rlib, retaining the normal panic-abort profile.
At final link, a Rust dylib reexports that exact rlib and its dependency graph;
the stdlib imports its symbols instead of including a second runtime archive.
"""
import os
from pathlib import Path
import re
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
    command = [
        os.environ.get("RUSTC", "rustc"), "--edition=2021",
        "--crate-name", "perry_provider_runtime", "--crate-type=dylib",
        "-C", "panic=abort", "-C", "opt-level=2", "-C", "codegen-units=16",
        "-C", "force-unwind-tables=yes", "-Z", "use-sync-unwind=yes",
        "-C", "linker=" + real_cc,
        *externs, str(source), "-o", str(library),
    ]
    # Cargo's artifact layout can put every dependency in a separate directory.
    # Resolve by the original crate metadata, never by a guessed archive glob.
    directories = dict.fromkeys(str(Path(a).parent) for a in arguments if a.endswith(".rlib"))
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
    subprocess.run(command, check=True)


def main():
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
