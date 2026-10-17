#!/usr/bin/env python3
"""Run a Perry fixture with its optional process address-space ceiling.

Metadata: // parity-rlimit-as-mb: <positive integer MiB>
Usage: parity_fixture_limits.py [--print-limit] fixture.ts [command ...]
The oracle and compilation must not use this execution-only ceiling.
Unsupported containment refuses execution (exit 125), never an uncapped PASS.
"""
import argparse
import os
from pathlib import Path
import re
import sys


def limit_mb(text):
    directives = re.findall(r"^[ \t]*//[ \t]*parity-rlimit-as-mb:[ \t]*(.*)$", text, re.M)
    if not directives:
        return None
    directives = [value.strip() for value in directives]
    if len(directives) != 1 or not re.fullmatch(r"[1-9][0-9]{0,12}", directives[0]):
        raise ValueError("require one positive integer parity-rlimit-as-mb directive")
    value = int(directives[0])
    if value > (2**63 - 1) // (1024 * 1024):
        raise ValueError("address-space limit exceeds the supported byte range")
    return value


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--print-limit", action="store_true")
    parser.add_argument("fixture", type=Path)
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    try:
        value = limit_mb(args.fixture.read_text(encoding="utf-8"))
    except (OSError, ValueError) as exc:
        print(f"{args.fixture}: {exc}", file=sys.stderr)
        return 2
    if args.print_limit:
        print(value if value is not None else "")
        return 0
    if not args.command:
        parser.error("command is required unless --print-limit is used")
    if value is not None:
        try:
            import resource
            requested = value * 1024 * 1024
            existing = resource.getrlimit(resource.RLIMIT_AS)
            ceiling = min([requested] + [n for n in existing if n != resource.RLIM_INFINITY])
            resource.setrlimit(resource.RLIMIT_AS, (ceiling, ceiling))
            actual = resource.getrlimit(resource.RLIMIT_AS)
            if any(n == resource.RLIM_INFINITY or n > requested for n in actual):
                raise ValueError("limit readback did not confirm containment")
        except (ImportError, AttributeError, OSError, ValueError) as exc:
            print(f"{args.fixture}: cannot enforce address-space limit ({value} MiB): {exc}",
                  file=sys.stderr)
            return 125
    os.execvp(args.command[0], args.command)


if __name__ == "__main__":
    raise SystemExit(main())
