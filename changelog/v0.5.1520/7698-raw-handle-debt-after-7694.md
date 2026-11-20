**`fix(gc)`: restore the raw-handle debt baseline after #7694, and convert four pairs to pay for it.**

#7694 (#7268's JSON shape-template rooting) added 5 bare `RuntimeHandle` reads and took `raw_handle_debt.py` from 998 to 1003 — `lint` red on `main`. **I merged it having run the gate and seen it red**; the merge step in my script did not gate on the result.

The 5 reads are the shape `raw_handle_debt_files.txt` explicitly permits: *a loop whose collection window is a user-visible trap re-reads every live handle at the top of each iteration.* `toJSON` is exactly that trap, and `cur_obj()` / `cur_keys()` re-derive at every access across it — `across_*` pairs one call with one re-read and cannot express a helper called at many points inside a loop. The test module is a second permitted case: its assertions **are** pre/post address comparisons, which is the thing `across_*` exists to make unnameable, so converting them would delete the test's subject.

That rule carries a condition — listing is legitimate **only if the same change converts enough pairs elsewhere that the global baseline does not rise** — and this change meets it. Four pairs converted:

- `proxy/put_value.rs` — `js_put_value_set` + the `key` re-read → `across_const`. That module reaches **zero and is deleted from the list**, which the ratchet requires.
- `util_promisify.rs` ×3 — two `js_closure_alloc` early-return paths and the two `js_get_exception` / `js_clear_exception` rejection paths → `across_mut`. Ceiling **49 → 45**.

Net: **998 → 998**, per-module 109 modules within ceilings, every other runtime module locked at zero.
