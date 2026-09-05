**Gate follow-ups for train125.** Two of these are gates pinning a *spelling*
where the guarantee is a *property*; both were sabotage-checked after widening,
so the guarantee is unchanged.

- `tdz_numeric_const_read_is_not_constant_folded` pinned
  `call i64 @js_box_get_bits(...)`. #9721 routes the read through
  `js_box_get_bits_named`, which additionally passes the binding name so the
  thrown `ReferenceError` can identify it — a strictly better error, which the
  test read as a lost guard. It now accepts either helper and additionally
  asserts the read is NOT folded to the later value, which it never checked.
- `shape_descriptor_census` required `family_push_back` after
  `slab_mut().insert`. #9768 added `family_append_fresh` — the same append minus
  a membership scan that is dead work for an id `alloc_shape_id` just minted and
  never reuses. The census now accepts either append and still enforces the
  ordering: the by-id descriptor must exist before the reverse accelerator
  points at it.
- `hot_diag.rs` and `alloc_census.rs` moved to `perry_thread_local!`, which also
  made their holders visible to `gc_runtime_root_holders` (the #9740 design);
  `CREDIT` and `LAST_IDLE_PREDICTED_RELEASE` are classified as counters.
- `intl/segmenter.rs`'s shared keys array is built inside `with_mut_ptr`, with
  every use — including publication into `SEGMENT_RECORD_KEYS`, which #9769
  registers a scanner for — inside the scope.
- `PASS1_MARKED`'s window re-pinned after #9769 and #9771 touched pinned files.
  Decisive: `census_take_if_armed_at_full_sweep_start` takes the snapshot out of
  the thread-local BEFORE calling `take_census`, so #9771's feature-gated
  Rust-heap dump inside it runs after the window has closed.
- Two `page_meta.rs` band literals are allowlisted as what they are: synthetic
  block ranges inside a `#[test]`, not runtime classification.
