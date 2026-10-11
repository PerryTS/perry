#!/usr/bin/env python3
"""Record hot ELF functions from one representative Linux run, including Rust.

Compile with PERRY_KEEP_SYMBOLS=1 (or --record-function-order, which also
records exact generated first calls). Profiling is entirely outside the
application: the consuming executable has no sampling or first-call hooks.

Example:
  PERRY_KEEP_SYMBOLS=1 perry compile app.ts -o app
  scripts/record_function_order.py -o app.order -- ./app representative-args
  perry compile app.ts --function-order app.order -o app.hot
"""
import argparse
import collections
import os
from pathlib import Path
import subprocess
import tempfile
import sys


def alias_groups(symbols):
    """Group all names of each physical body, including repeated local names."""
    parent = {}
    def root(name):
        parent.setdefault(name, name)
        while parent[name] != name:
            parent[name] = parent[parent[name]]
            name = parent[name]
        return name
    at_address = {}
    sizes = {}
    for address, size, name in symbols:
        key = root(name)
        if address in at_address:
            parent[root(at_address[address])] = key
        at_address[address] = name
        sizes[address] = max(size, sizes.get(address, 0))
    names = collections.defaultdict(list)
    for name in parent:
        names[root(name)].append(name)
    groups = {key: tuple(sorted(value)) for key, value in names.items()}
    by_name = {name: group for group in groups.values() for name in group}
    group_sizes = collections.Counter()
    for address, name in at_address.items():
        group_sizes[by_name[name]] += sizes[address]
    return by_name, group_sizes


def select_symbols(samples, defined, coverage, aliases=None):
    """Deterministic descending-period order, resolving only real ELF names."""
    counts = collections.Counter()
    for name, period in samples:
        if name in defined:
            counts[aliases[name][0] if aliases else name] += period
    total = sum(counts.values())
    selected = []
    covered = 0
    for name, period in sorted(counts.items(), key=lambda item: (-item[1], item[0])):
        selected.extend(aliases[name] if aliases else [name])
        covered += period
        if covered >= total * coverage:
            break
    return selected, covered, total


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("-o", "--output", required=True, type=Path)
    parser.add_argument("--coverage", type=float, default=0.90,
                        help="fraction of executable samples to pack (default: .90)")
    parser.add_argument("--repeat", type=int, default=1,
                        help="repeat short programs to collect enough samples")
    parser.add_argument("--event", default="cycles:u")
    parser.add_argument("--frequency", type=int, default=997)
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    command = args.command
    if command and command[0] == "--":
        command = command[1:]
    if not command or not 0 < args.coverage <= 1 or args.frequency <= 0 or args.repeat <= 0:
        parser.error("supply a command, 0 < coverage <= 1, and a positive frequency")
    binary = Path(command[0]).resolve()
    nm = subprocess.run(["nm", "-S", "--defined-only", str(binary)],
                        check=True, capture_output=True, text=True)
    defined = {}
    symbols = []
    for line in nm.stdout.splitlines():
        parts = line.split()
        if len(parts) == 4 and parts[2] in ("t", "T", "W", "w") and int(parts[1], 16):
            defined[parts[3]] = int(parts[1], 16)
            symbols.append((int(parts[0], 16), int(parts[1], 16), parts[3]))
    if not defined:
        parser.error("binary has no text symbols; rebuild with PERRY_KEEP_SYMBOLS=1")
    with tempfile.TemporaryDirectory(prefix="perry-function-order-") as tmp:
        data = str(Path(tmp) / "perf.data")
        run_command = command
        if args.repeat > 1:
            run_command = [
                sys.executable, "-c",
                "import subprocess,sys; "
                "[subprocess.run(sys.argv[2:],check=True) for _ in range(int(sys.argv[1]))]",
                str(args.repeat), *command,
            ]
        subprocess.run(["perf", "record", "-q", "-e", args.event,
                        "-F", str(args.frequency), "-o", data, "--", *run_command], check=True)
        profile = subprocess.run(
            ["perf", "script", "-i", data, "--no-demangle",
             "-F", "period,ip,sym,dso", "--hide-call-graph"],
            check=True, capture_output=True, text=True,
        )
    samples = []
    for line in profile.stdout.splitlines():
        parts = line.strip().split(None, 3)
        if len(parts) != 4:
            continue
        period, _ip, name, dso = parts
        try:
            weight = int(period)
        except ValueError:
            continue
        # Exclude identically named symbols in shared libraries.
        if os.path.realpath(dso.strip("()")) == str(binary):
            samples.append((name, weight))
    aliases, group_sizes = alias_groups(symbols)
    names, covered, total = select_symbols(samples, defined, args.coverage, aliases)
    if not names:
        parser.error("no executable samples resolved; run longer or raise --frequency")
    bodies = {aliases[name] for name in names}
    size = sum(group_sizes[body] for body in bodies)
    header = (
        "# perry ELF hot order: sampled generated + runtime + stdlib functions\n"
        f"# coverage={covered / total:.6f} symbols={len(names)} bodies={len(bodies)} bytes={size}\n"
    )
    lines = []
    for name in names:
        if name == aliases[name][0]:
            lines.append("# body: aliases below share existing code")
        lines.append(name)
    args.output.write_text(header + "\n".join(lines) + "\n")
    print(f"{len(names)} functions, {size / 2**20:.2f} MiB, "
          f"{covered / total:.1%} of executable samples", flush=True)


if __name__ == "__main__":
    main()
