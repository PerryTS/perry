**`fix(gc)`: restore the raw-handle debt baseline after #7693, and convert four pairs to pay for it.**

#7693's regression test asserts a sentinel's **pre/post address comparison** — which is precisely what `across_*` exists to make unnameable, so converting its four reads would delete the test's subject. It joins `json_shape_template.rs` as the second instance of that permitted shape.

`raw_handle_debt_files.txt` allows listing only when the same change converts enough pairs elsewhere that the global baseline does not rise. Four converted, each a genuine read-after-allocating-call:

- `os.rs` — `js_object_alloc_with_shape` + the `times` re-read (ceiling 4 → 3)
- `util_parse_args.rs` — `js_string_from_bytes` + the `values` re-read (20 → 19)
- `weakref.rs` — `entries_array` (itself allocating) + the `entry` re-read (7 → 6)
- `regex/match_all.rs` — `build_match_all_groups_owned` + the inner-array re-read (14 → 13)

Net **998 → 998**, 110 modules within ceilings.

Also converts three `weakref.rs` `js_string_from_bytes` + registry-re-read sites to `across_nanbox`. Those do not move the counter — it matches only `.get_raw_{mut,const}_ptr` — but they are the same defect and the same fix, and leaving them because they are invisible to the ratchet would be optimising for the metric.
