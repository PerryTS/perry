# Shape-descriptor census: the audit inventory

Baseline: `origin/main` @ `592f536dda`, 2026-10-30. Scope:
`scripts/shape_descriptor_census.py` and `scripts/shape_descriptor_census_baseline.json`.
This document changes no gate data.

## Why this exists

The census detects drift by raising `CensusError`, and `main()` let it escape, so
every detection arrived as a Python traceback with the finding buried in the
middle. Because the census stacks dozens of independent assertions, each run also
revealed only the *first* one: the layers behind it were invisible and hard to
reach. #13533 made the failures read as one-line verdicts.

This is the inventory that readability unlocked. It says what is wrong, which
parts are already understood and verified, and what remains — so the work does not
have to start from the top again.

## Method: collect instead of abort

`require_code` is the census's own assertion helper. Wrapping it records every
failure in one pass instead of stopping at the first:

```python
import sys; sys.path.insert(0, "scripts")
import shape_descriptor_census as C
failures = []
orig = C.require_code
def patched(body, pattern, message):
    try: orig(body, pattern, message)
    except C.CensusError: failures.append(message)
C.require_code = patched
try:
    C.assert_authority_surfaces(C.rust_sources()); direct = None
except C.CensusError as e:
    direct = str(e)
print(len(failures), failures, direct)
```

One run, before any fix, returned **6 `require_code` failures and 1 direct raise**.
That is the whole stack: not the 77 assertions the function contains, but the 7
that are stale. All of them trace back to two refactors — `#13443` (const-generic
`DIAGNOSTICS`, which changed call-site spellings) and the A1 object-header work
(which moved the canonical GC-kind values into `perry-abi`).

## The inventory

**A. The baseline comparison** (`compare_exact_census`, evaluated before the
authority surfaces, so it masks everything below it):

| Change | Cause | State |
|---|---|---|
| `raw_member_callsite_multiset`: one `keys_array` **declaration** gone from `object/mod.rs` | `2c36490eba` "build RuntimeState in place" | solved |
| `codegen_object_header_size_callsite_multiset`: all 32 `object_header_size_bytes` call sites gone; summary 47 → 0 | A1 replaced the single accessor with **two**: `object_slot_base_bytes` (50 uses) and `object_meta_slot_offset_bytes` (8) | solved |

**B. `assert_authority_surfaces`** (6 `require_code` + 1 direct raise):

| # | Assertion | Root cause | State |
|---|---|---|---|
| 1 | `layout dispatcher reaches visit_object_layout_slots` | `#13443` made the callee `<const DIAGNOSTICS: bool>`; the pattern pinned the pre-generic spelling | solved |
| 2 | `layout dispatcher reaches visit_gc_layout_slot_descriptors_generic` | same, second callee — one layer hid the other | solved |
| 3 | `RULE3_KINDS names GC kinds that no longer exist` (direct raise) | GC kinds are declared four ways since A1; only two were recognised | solved |
| 4 | `keys-scoped descriptor lineage index` | `retire_owned_shape_siblings` now lives at `object/shapes.rs:4475` and indexes `owned_families: PtrHashMap<u64, IdList>`; the pinned `families.get(&keys)` spelling is gone | diagnosed, needs the new spelling |
| 5 | `object_is_regular GC kind` | the body no longer compares literally; it calls `crate::gc_type_is_object!(header.obj_type)` (`object/mod.rs:1750`) | diagnosed |
| 6 | `object_is_shaped GC kind` | same macro form | diagnosed |
| 7 | `method key lookup is bounded by descriptor count` | `object_shape_record` is at `object/shapes.rs:5625` and `key_position` at `object/native_call_method/class_holder.rs:76` — the second one is outside the pinned `field_set_by_name/tail.rs` body the census inspects | diagnosed |

Zero items remain undiagnosed.

## The verified fixes, so they are not re-derived

1. **Turbofish (#1, #2).** `require_code` pattern, using the same idiom
   `raw_handle_debt.py` uses for `across_*`:
   `rf"\b{callee}\b(?:::\s*<[^;{{}}]*>)?\s*\(header,\s*visit\)"`. The claim is
   unchanged: the dispatcher must still call `callee(header, visit)`.

2. **GC-kind declaration forms (#3).** `gc/types.rs` now carries four shapes:
   a literal, a codegen ABI constant
   (`pub const GC_TYPE_TYPED_ARRAY: u8 = crate::codegen_abi::BYTES_TYPE_BASE;`),
   an alias to another kind (`= perry_abi::GC_TYPE_OBJECT;`), and a re-export
   (`pub use crate::codegen_abi::GC_TYPE_OBJECT_META;`). The value arm must be the
   **union** of all of them plus a `pub use` sweep:

   ```python
   declared = {name for name in re.findall(
       r"pub\s+const\s+(GC_TYPE_\w+)\s*:\s*u8\s*=\s*"
       r"(?:0x[0-9A-Fa-f]+|\d+|crate::codegen_abi::\w+|(?:\w+::)*GC_TYPE_\w+)\s*;",
       gc_types)}
   declared |= set(re.findall(r"pub\s+use\s+(?:\w+::)*(GC_TYPE_\w+)\s*;", gc_types))
   declared -= {"GC_TYPE_MAX"}
   ```

   **Trap worth recording:** the first attempt wrote the alias arm as a
   *replacement*. It fixed the two missing kinds and broke
   `GC_TYPE_TYPED_ARRAY`, because `crate::codegen_abi::BYTES_TYPE_BASE` does not
   end in `GC_TYPE_*`. The arm has to be additive. With the union, the direct
   raise disappears entirely (`raise directo: NINGUNO`).

3. **Codegen accessors (baseline).** Track both, never one — covering a single
   accessor would silently halve what the census can see:

   ```python
   CODEGEN_HEADER_ACCESSORS = re.compile(
       r"\b(?:object_slot_base_bytes|object_meta_slot_offset_bytes)\b")
   ```

   plus the same two names in `observed_census`'s prefilter, then regenerate with
   `--emit-baseline`. Verified arithmetic: 58 raw mentions in `crates/perry-codegen/src`,
   55 counted as code, 3 dropped as comments — the census counts code, not comments.

## What remains before this census can be trusted

Items #4–#7, each needing its guarantee re-checked (not merely its regex
loosened), then the regenerated baseline, then verification in both directions.
There is no partial green: `compare_exact_census` and every assertion run in one
`main()`, so the census stays red until the last one is done. That is why the
verified fixes above are recorded rather than shipped as a half-fix — a PR that
refreshes the baseline while four assertions are still stale would look like a
census that was audited.

## Reproducing

```bash
python3 scripts/shape_descriptor_census.py          # one clean ::error:: line, exit 1
python3 scripts/shape_descriptor_census.py --emit-baseline > /tmp/observed.json
# then the collection snippet above to see all seven at once
```

## What this document does not do

It does not change the baseline, the assertions, or the census rule, and it does
not claim the census is trustworthy today. It records the state so the remaining
four items can be finished without rediscovering the seven.
