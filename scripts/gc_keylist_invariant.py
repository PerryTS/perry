"""Structural custody gate for the scanner-free, weak canonical trie (I3).

The trie is a shape authority, not a root. Its addresses are born old and
movement-pinned, and its full-trace prune runs before sweep can reuse storage.
The contract rejects a removed old birth, movement exclusion or death prune.
"""
from pathlib import Path
import re
from check_gc_scanner_latches import mask_non_code


def keylist_contract_problems(entry: dict, root: Path | None) -> list[str]:
    if root is None:
        return ["weak_pinned_old requires a repository root"]
    if (entry.get("file"), entry.get("name")) != (
        "crates/perry-runtime/src/object/canonical_keys.rs", "CANONICAL_KEYS"
    ):
        return ["weak_pinned_old requires an audited holder contract"]
    prefix = "crates/perry-runtime/src/"
    requirements = {
        "object/canonical_keys.rs": [r"alloc_canonical_key_list\(", r"fn prune_dead_canonical_keys\("],
        "object/key_attrs.rs": [r"fn alloc_canonical_key_list\(", r"js_array_alloc_canonical_keys\("],
        "array/alloc.rs": [r"arena_alloc_gc_old_born_tenured\(bytes, 8, crate::gc::GC_TYPE_ARRAY\)",
                           r"if canonical\s*\{\s*\(\*header\)\.gc_flags \|= crate::gc::GC_FLAG_SHAPE_SHARED;"],
        "gc/oldgen.rs": [r"flags & \(GC_FLAG_FORWARDED \| GC_FLAG_PINNED \| GC_FLAG_SHAPE_SHARED\) == 0"],
        "gc/dead_owner.rs": [r"prune: crate::object::canonical_keys::prune_dead_canonical_keys"],
    }
    problems = []
    for file, patterns in requirements.items():
        text = mask_non_code((root / (prefix + file)).read_text())
        for pattern in patterns:
            if not re.search(pattern, text):
                problems.append(f"I3 weak_pinned_old contract missing {file}: {pattern}")
    canonical = mask_non_code((root / (prefix + "object/canonical_keys.rs")).read_text())
    if re.search(r"key_attrs::alloc_key_list\(", canonical):
        problems.append("I3 canonical backing uses a movable nursery allocator")
    return problems


def keylist_contract_self_test(root: Path) -> list[str]:
    import tempfile
    import shutil
    entry = {"file": "crates/perry-runtime/src/object/canonical_keys.rs", "name": "CANONICAL_KEYS"}
    failures = []
    with tempfile.TemporaryDirectory() as tmp:
        copied = Path(tmp)
        files = ["object/canonical_keys.rs", "object/key_attrs.rs", "array/alloc.rs", "gc/oldgen.rs", "gc/dead_owner.rs"]
        prefix = "crates/perry-runtime/src/"
        for file in files:
            dest = copied / (prefix + file)
            dest.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(root / (prefix + file), dest)
        if keylist_contract_problems(entry, copied):
            failures.append("I3 contract rejects the production invariant")
        for file, before, after in [
            ("array/alloc.rs", "arena_alloc_gc_old_born_tenured(bytes", "arena_alloc_gc(bytes"),
            ("array/alloc.rs", "gc_flags |= crate::gc::GC_FLAG_SHAPE_SHARED", "gc_flags |= 0"),
            ("gc/oldgen.rs", "GC_FLAG_FORWARDED | GC_FLAG_PINNED | GC_FLAG_SHAPE_SHARED", "GC_FLAG_FORWARDED | GC_FLAG_PINNED"),
            ("gc/dead_owner.rs", "prune: crate::object::canonical_keys::prune_dead_canonical_keys", "prune: omitted"),
        ]:
            path = copied / (prefix + file)
            original = path.read_text()
            assert before in original, (file, before)
            path.write_text(original.replace(before, after))
            if not keylist_contract_problems(entry, copied):
                failures.append(f"I3 negative control missed {file}: {before}")
            path.write_text(original)
    return failures
