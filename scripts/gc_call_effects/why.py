#!/usr/bin/env python3
"""Print why a runtime symbol has its generated GC-effect class.

    why.py --archive libperry_runtime.a --archive libperry_stdlib.a SYMBOL...

For each symbol: its class, and for each fixed point (L2 = Leaf, L2b =
AllocOnly, L2b_throw = ThrowOnly) the BFS-shortest path from the symbol to the
seed that taints it, one function per line, ending at the seed and its kind
(named seed, `indirect_call @+0xOFF insn`, `extern:NAME`). A thin wrapper over
`callgraph.py why`.
"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import callgraph  # noqa: E402

if __name__ == "__main__":
    sys.exit(callgraph.main(["why"] + sys.argv[1:]))
