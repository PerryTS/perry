### Fixed

- **The `typeof` string-cache rooting test drove six of the eight cache
  cells.** `scan_typeof_string_roots_mut` is eight hand-written `visit(...)`
  calls, one per interned `typeof` result, so `TYPEOF_BIGINT` and
  `TYPEOF_SYMBOL` could have lost theirs and no test would have noticed.
  `test_gap_gc_typeof_string_cache_rooting.ts` now drives all eight.

  Measured by unregistering the scanner and rebuilding, rather than assumed:

  | | default | `POLLS=1`, compiled *and* run with the flag |
  |---|---|---|
  | registered | `bad 0` | `bad 0` 5/5 |
  | unregistered | — | `bad 592` 5/5 |

  The six-cell version of this test reported `bad 444`, and
  `592 / 8 == 444 / 6 == 74` — the two added cells go bad at the same
  collection as the other six, which is what says they are really covered
  rather than decorative.

### Added

- **Rust-side mark, rewrite and registration tests for both interned-string
  root scanners** (`gc/tests/runtime_roots/interned_string_caches.rs`):
  `builtins::arithmetic::scan_typeof_string_roots_mut` and
  `json::raw_json::scan_raw_json_key_root_mut`. Neither had one; #7211
  registered them and the `.ts` gap test covered only the `typeof` side, from
  one direction, at six cells.

  Marking and rewriting are asserted separately on purpose. Marking alone
  keeps the string alive but still hands out a pre-move address after a
  copying minor, which is the #7211 failure in full — the distinction
  `docs/src/internals/gc-rooting-invariant.md` keeps having to make. The
  registration test is separate again, because either scanner can be called
  directly from a test whether or not `gc_init` ever names it, and an
  unregistered scanner is a no-op in production.

  Sabotage-tested, per the project's own rule that a gate must be shown able
  to fail:

  | sabotage | result |
  |---|---|
  | drop `visit(&TYPEOF_BIGINT, visitor)` | mark and rewrite tests red, naming `cell 6` |
  | drop the `gc_init` registration of `scan_raw_json_key_root_mut` | registration test red |

### Changed

- **`reset_typeof_string_cache_for_test` was dead code.** It had no callers,
  and its doc comment described a shared arena-reset teardown that does not
  exist in this repo — every other `_for_test` helper in `perry-runtime` is
  called. It is now driven by the tests above, and its eight-cell list is
  shared with the new `populate_*` / `*_cells_for_test` helpers instead of
  being written out a second time. `json/raw_json.rs` gets the matching trio
  (`reset_`, `populate_`, `peek_`), which is what made the rawJSON scanner
  testable at all.

  All `#[cfg(test)]`; no runtime behavior changes.

- **`CLAUDE.md`: the two #7226 entries are folded back toward the length of
  the entries around them**, 2006 → 1722 and 1358 → 903 characters, in a
  section whose other bullets run 242-355. The file's own opening note says
  to keep it concise and put detail in `changelog.d/`. Nothing operational
  was dropped: the incident narrative is already in
  `changelog.d/7219-registry-gc-unrooted-caches.md`, and the detector knobs
  the new bullet re-listed (`PERRY_GC_ZEAL`, `PERRY_GC_PROTECT_FROMSPACE`,
  `PERRY_GC_PROTECT_FROMSPACE_DEPTH`) are documented in full, with their
  exact gating, two sections above under "Rooting-bug instruments".

- **`changelog.d/7214-closure-calln-stale-registers.md` line 117 opened with
  `#7161`**, which markdownlint reads as a malformed ATX heading (MD018).
  Rewrapped so the reference is not the first thing on the line. The
  rendered text is unchanged — CommonMark requires a space after `#`, so it
  was never a heading, and the line is a paragraph continuation besides.

## Not changed, and why

**`SYMBOL_ROOTS` in `scripts/gc_root_dominance_check.py` does not need the
`crates/perry-ext-*` crates.** `--audit-alloc-re` is a liveness check on
`ALLOC_RE`'s alternatives — it asks whether each alternative matches at least
one real exported symbol — so widening the symbol corpus can only make it
more permissive, never less. Measured: 3775 symbols under the current two
roots, 394 more that exist only in the 38 ext crates, and the dead-alternative
verdict is the empty list with or without them. No alternative is kept alive
only by an ext symbol. The 26 ext-only allocating symbols already match
`ALLOC_RE` through the `_new` / `_create` conventions, so what the checker
detects is unchanged either way.

A symbol that allocates and matches no alternative would be a real hole, but
it is a hole in `ALLOC_RE` and this audit runs the other direction, so adding
roots would not surface it.
