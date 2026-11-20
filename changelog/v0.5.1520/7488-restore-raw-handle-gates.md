### Gates: raw-handle debt and file-size regressions from the #7467 batch restored

The #7467/#7470/#7471/#7474 review batch was merged against a stale
`origin/main` (`git fetch origin <branch>` does not update `origin/main`), so
its gate check ran green while true `main` went red: the raw-handle total hit
1010 vs the 999 baseline, `array/indexing.rs` (7 > 6) and
`object/descriptors.rs` (17 > 12) exceeded their ceilings,
`object/field_get_set/enumeration.rs` carried 5 bare reads with no ceiling,
and `array/indexing.rs` crossed the 2000-line cap (2005).

Restored without weakening anything:

- **Eleven canonical pairs converted to `across_*`** (descriptors ×5,
  indexing ×1, regex/exec ×1, regex/match_string ×2, regex.rs ×1, map.rs ×1)
  — total back to exactly 999, descriptors at its 12 ceiling, indexing at 6.
  Ceilings ratcheted down on every touched module.
- **`enumeration.rs` listed under a new, documented exception** in the
  `raw_handle_debt_files.txt` header: a loop whose collection window is a
  user-visible trap call (here the Proxy values/entries interleave — ownKeys,
  then per-key gOPD + get) re-reads every handle per iteration; that re-read
  IS the #7341 discipline and has no `across_*` form. Listing such a module is
  legitimate only when the same change converts enough pairs elsewhere that
  the global baseline does not rise — which this change does.
- **`array/indexing.rs` split**: the four bulk-fill entry points
  (`js_array_fill_f64_{const,iota}[_len]_extend`) and their keepalive anchors
  moved to `array/fill_extend.rs` (all reached by emitted code only; symbol
  names unchanged). 1757 + 262 lines, both under the cap.
