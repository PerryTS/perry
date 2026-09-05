**Gate follow-ups for the 20-PR train.** Five gates went red together; each is
fixed at the source rather than baselined away.

- `ic_miss.rs`'s new `#[cold]` IC-miss diagnostic guarded a key pointer with a
  bare `< 0x1000` floor, which admits the whole handle band — real addresses on
  Linux, hidden by macOS's higher mmap base (#9219 class). It now asks
  `addr_class::is_above_handle_band`.
- Six raw-handle reads in `bun_compat/jsc.rs`, `class_registry/construct.rs` and
  `construct/class_object.rs` moved into `with_mut_ptr` (#7341).
- `IC_DIAG` and `REGEX_DIAG` are classified. Both are off-by-default diagnostics;
  `REGEX_DIAG`'s entry records that its `per_pattern` key is a pattern
  `StringHeader` address used only as an opaque grouping id — never
  dereferenced — and that a recycled address merges two rows' counters.
- `PASS1_MARKED`'s `non_moving_snapshot` pin was re-audited after #9760 touched
  `gc/mod.rs`. That change is `mod heap_stats;` plus a re-export and alters no
  mark/sweep control flow; `heap_stats()` runs only from the JS-facing
  `bun:jsc.heapStats()`, never inside a cycle. The window is unchanged, so only
  that one file's digest was re-pinned.
- The string payload-access ratchet moved the good way (358 → 353 sites) and its
  baseline is recorded.

`regex.rs` also crossed the 2000-line cap, so `escape_regexp_source` moved to the
existing `regex/escape.rs` and flags validation to a new `regex/flags.rs`, both
gated on `regex-engine` like their siblings.
