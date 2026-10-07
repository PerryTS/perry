#!/usr/bin/env python3
"""Mutant for test-files/test_gap_fs_numeric_fd_paths.ts: classify a buffer
probe word by stripping ANY tag (the pre-fix shape) instead of by tag first,
so a numeric fd reaches a header read. After rebuilding perry, perry-runtime,
perry-runtime-static and perry-stdlib-static the gap test must crash or
differ from node. Prints a unified patch on stdout; apply it in a disposable
worktree and use `git apply --reverse` to restore.
"""
import difflib
from pathlib import Path

path = "crates/perry-runtime/src/buffer/query.rs"
repo = Path(__file__).resolve().parents[1]
text = (repo / path).read_text()
before = """    let addr = if (bits & crate::value::TAG_MASK) == crate::value::POINTER_TAG {
        bits & crate::value::POINTER_MASK
    } else if (bits >> 48) == 0 {
        bits
    } else {
        return None;
    };"""
after = """    let addr = if (bits >> 48) != 0 {
        bits & 0x0000_FFFF_FFFF_FFFF
    } else {
        bits
    };"""
if text.count(before) != 1:
    raise SystemExit("anchor must occur exactly once")
new = text.replace(before, after, 1)
diff = difflib.unified_diff(text.splitlines(True), new.splitlines(True), f"a/{path}", f"b/{path}")
print("".join(diff), end="")
