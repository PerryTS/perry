### Fixed

- **`lint` is green again: two files had crossed the 2000-line cap on `main`.**
  `crates/perry-hir/src/lower/pre_scan.rs` went 1985 → 2011 in #7828 and
  `crates/perry-runtime/src/gc/layout.rs` went 1982 → 2010 (#7809) → 2023
  (#7812), so `scripts/check_file_size.sh` — and with it the whole `lint` job —
  failed on every commit after that batch.

  Both are pure code moves with no logic change:

  - `pre_scan_weakref_locals` and its doc comment move to
    `lower/pre_scan/weakref_locals.rs`, leaving 1653 lines behind. It is the
    natural unit: one of the file's three top-level pre-scans, owning its own
    five local-name sets, and the one #7828 grew.
  - `LayoutSlotMask` — the enum and its entire `impl` — moves to
    `gc/layout/slot_mask.rs`, leaving 1807 lines behind. Its visibility widens
    from `pub(super)` to `pub(in crate::gc)` only because the type now sits one
    module deeper while `layout_tables.rs` and `hot_tls.rs` still name it; the
    set of modules that can reach it is unchanged.

  The cap is cheap to check and expensive to notice late — `bash
  scripts/check_file_size.sh` before pushing is the whole remedy, and it is
  already written down in CLAUDE.md's "CI gates that surprise people".
