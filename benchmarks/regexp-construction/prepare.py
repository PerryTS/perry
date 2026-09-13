#!/usr/bin/env python3
"""Resolve the issue #10179 probe against an existing OpenCode Bun install."""
import os
from pathlib import Path
import sys

npm = Path(os.environ["OPENCODE_SRC"]) / "node_modules/.bun"
source = Path(__file__).with_name("probe.ts.in").read_text()
target = Path(sys.argv[1])
target.write_text(source.replace("@NPM@", str(npm)))
print(target)
