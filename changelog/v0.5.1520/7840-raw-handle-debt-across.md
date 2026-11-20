### Fixed

- **The raw-handle debt ratchet is green again on `main` (#7838).** It went red at
  1,013 bare `get_raw_{mut,const}_ptr` reads against a baseline of 998, with four
  per-module violations. The +15 all arrived with the two #6949 rooting fixes in the
  2026-08-11 batch (#7811, #7815); #7825 — same batch — closed the hole that had been
  letting a PR's own checkout carry the comparison baseline, so the ratchet only
  started seeing them once it landed. While a required gate is red on `main`, no agent
  can tell its own breakage from inherited breakage, which is the condition a real
  regression merges unnoticed in.

  **All fifteen convert to `RuntimeHandle::across_{mut,const}`. No ceiling was raised
  and `raw_handle_debt_baseline.txt` is untouched** — the total lands back on 998
  exactly, and `regex/replace_fn.rs` returns to its recorded ceiling of 3.
  `disposable.rs`, `messaging.rs` and `builtins/formatting/boxed_primitives.rs` reach
  zero and stay unlisted.

  **`replace_fn.rs` did not need the 3 → 13 raise #7838 proposed.** The proposal rested
  on all thirteen being the one shape `raw_handle_debt_files.txt` sanctions joining the
  list for — a loop whose collection window is a user-visible callback, where a
  `cur_str` helper re-derives at every access and `across_*` (one call ↔ one re-read)
  cannot express it. That describes **three** of them, and those three are
  *pre-existing*: `git show 6af7e5840^:…/replace_fn.rs | grep -c get_raw_` is 3, which
  is what the ceiling of 3 was recorded for. #7811 added **ten**, all of them plain
  root → one allocating `js_string_coerce` → read-for-the-call. Four are two-receiver
  sites; two receivers compose by **nesting** `across_const`, the same way
  `path::value_args::with_two_headers` already does it, and two small private
  combinators carry them.

- **`SuppressedError` filed its property attributes under a possibly-stale address.**
  `set_nonenum` called `object::set_property_attrs(obj as usize, …)` *after*
  `js_object_set_field_by_name`, which allocates when the object grows. That side table
  is keyed on the address, so a pre-call copy does not fault — it files the attributes
  where nothing will look them up, and `error` / `suppressed` / `message` silently
  become **enumerable** on a `SuppressedError` that grew during the set.

- **`js_suppressed_error_new` returned a NaN-box built before its last allocating
  call.** `js_nanbox_pointer(obj)` was computed, then the `SuppressedError.prototype`
  lookup ran, then the box was returned. A NaN-box is a frozen address the collector
  cannot rewrite, so the returned value named from-space if that lookup collected. The
  box is now built last.

  Both are the #7192 shape — the store is in-frame but *after* a call that allocates —
  and neither is reachable without evacuation actually moving the receiver, so this is
  ordering hygiene rather than an observed crash. They are recorded because writing the
  `across_*` ordering out is what made them visible, which is the argument for the
  discipline the ratchet enforces.
