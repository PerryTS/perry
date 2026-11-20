**Cleared the two `lint` steps that had been failing on `main` for days —
`gc_store_site_inventory.py` (19 unaudited GC store sites) and
`addr_class_inventory.py` (2 ratchet regressions plus a stale allowlist
substring).** Both were invisible in CI: the `lint` job stops at its first
failing step and step 9 fails ahead of them, so neither had ever executed.

Of the 19 GC store-site findings, **5 were phantom**. `classify_rust_store`
searched the atomic-store regexes against `call_window` — a 6-line *forward*
window — without anchoring, so a single `cache.store(...)` was re-reported from
each of the five lines above it; `continue;` and a bare `}` were both listed as
"raw atomic cache/global pointer store". `anchored_search` now requires the
match to begin inside the head line, preserving the split-call coverage the
window exists for (`CACHE.store(` … `);`) while reporting each site once.

Three were genuinely new runtime sites and got a class each: the `TAG_HOLE`
capacity fills in `array/alloc.rs` and `array/push_pop.rs` (#7138) are `INIT` —
freshly allocated, never-published storage written with a non-pointer sentinel,
so there is no old value to remember and no edge to record; the `cache.store` in
`array/indexing.rs`'s `scan_prototype_addr_cache_roots_mut` (#7071) is `ROOT` —
it *is* the collector rewriting a registered side-table slot with the mutator
stopped, and `visit_usize_slot` returns true only when it relocated the object.

The remaining 11 were codegen stores. Four already carried a
`GC_STORE_AUDIT(POINTER_FREE)` marker that #6915's `value_is_canonical_raw_f64`
branch split had pushed 9–10 lines below it, outside the ±6-line window. The
other seven had prose rationale but not the canonical marker form: the
`instance_misc1.rs` / `property_set.rs` field stores are reached only after
`emit_plain_finite_number_check` proves the value's exponent is *not* all-ones
(every NaN-box tag shares an all-ones exponent, so the value is a genuine
unboxed double), and the five `proven_view_access.rs` arms write the typed
array's backing store, whose elements are raw numeric bytes — the codegen-side
counterpart of the `is_pointer_free_module` carve-out the script already makes
for `typedarray`/`typedarray_view`/`buffer`. That half of the change is
comments only; no generated code and no runtime behaviour moved.

The address-classification failures were **real defects, not bookkeeping**.
Three hand-rolled address floors landed on 2026-07-30 without going through
`value::addr_class`: `> 0x10000` / `<= 0x10000` in
`child_process/value_util.rs` and `< 0x1000` in `fs/dirent.rs`.
`js_get_string_pointer_unified` forwards a `POINTER_TAG` payload verbatim and
`JSValue::as_pointer` does the same, and those payloads carry registry handle
ids as well as heap pointers — so a fetch (`0x40000..0xE0000`), zlib
(`0xE0000..0xF0000`) or proxy (`0xF0000..0x100000`) id passed a floor an order
of magnitude too low and was dereferenced as a `StringHeader`/`ObjectHeader`.
That is the Linux-only fault class of #1843 / #4004 / #6271, which macOS's high
allocation base hides. Every handle-floor site in both files was converted to
`is_handle_band` / `is_above_handle_band` — including the two grandfathered
ones, same defect and same one-line fix — so both baseline entries drop to zero
instead of being re-pinned lower. `is_handle_band` also subsumes the redundant
null checks that followed, and `cp_raw_slot_is_heap_ptr` keeps an explicit
`raw > 0` so a negative `i64` slot is not turned into a huge "above the band"
value by `as usize`.

Separately, the `O_SYMLINK` allowlist entry was keyed on one of the two
spellings in `native_module/constants.rs`, so `("O_SYMLINK", (0x200000) as
f64),` failed the gate while `"O_SYMLINK" => Some(0x200000),` was suppressed.
Re-keyed on the constant name; it remains path-prefixed to that file, so any
other band literal there still fails. The pre-split `object/native_module.rs`
entry was dropped — that file now holds zero band literals.

The ratchet baseline was regenerated: 566 → 544 sites across 256 → 249 entries,
verified to contain **zero increases and zero new keys** by diffing the
`(rule, path) → count` maps, so it is purely the slack that accumulated while
`lint` was red. Only 3 of the 22 come from this change.

Both new regression tests were confirmed to fail *without* the fix rather than
merely pass with it: the `proto_cache_scan` case fails on the pre-anchoring
logic with exactly the five phantom lines, and the `O_SYMLINK` case fails
against the pre-fix allowlist with exactly the uncovered spelling.

One finding was deliberately left open. `addr_class_inventory.py`'s
`SCAN_ROOTS` covers only `perry-runtime` and `perry-stdlib`, while its sibling
`gc_store_site_inventory.py` also globs `crates/perry-ext-*`. When #6826 moved
`perry-stdlib/src/http.rs` into the HTTP extension its 11 `handle-floor` sites
moved out of the gate's field of view rather than being fixed — 18 ext-crate
sites are unaudited, 8 of them `band-literal`, which is allowlist-governed and
would hard-fail until each is individually justified. Filed as #7272 rather
than silently absorbed. This change also does not by itself turn `lint` green:
step 9 (#7257) still fails ahead of these two.
