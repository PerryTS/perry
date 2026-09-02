### Fixed

- **The hole-leak family: perry's internal empty-slot sentinel stopped escaping
  into user-visible values.** `TAG_HOLE`'s bit pattern *is* a NaN, so any value
  ladder that falls through to "must be a regular number" turns a hole into
  `NaN` / `"number"`. Two producers were handing raw sentinels to user code, and
  four consumers had no arm for one.

  **Leak source 1 — `js_array_get_f64`'s Set/Map arm.** Perry deliberately
  exposes a `Set`/`Map`'s live elements through the same array-like indexed
  dispatch its `length` uses (`js_array_length` on a `Set` answers `size`), but
  that arm indexed the raw element buffer directly and bounded the read by the
  LIVE count while raw slots run `0..used`. After any `.delete()` it therefore
  returned the TOMBSTONE — with none of the hole translation the plain-array arm
  performs — *and* stopped short of the live tail:
  `typeof s[0] === "number"`, `String(s[0]) === "NaN"`, `` `${s[0]}` === "NaN" ``.
  It now reads through `js_set_value_at` / `js_map_entry_key_at`, the
  live-index accessors the collections' own walkers already use, which compact
  when `used != size` so raw index == live index again.

  **Leak source 2 — `Array.prototype.pop`.** Found while checking which
  downstream symptoms survived the first fix. The dense fast path explicitly
  DECLINES on a hole and falls through to the generic arm, which returned the
  raw slot: `[1, ,].pop()`, `new Array(3).pop()` and `delete a[a.length - 1]`
  then `pop()` all produced a value that was `typeof "number"`, stringified
  `"NaN"`, and compared `!== undefined`. That is the same shape #536 already
  fixed once in this very function for the empty-array case. The element READ
  arm translates the sentinel; the element REMOVE arm now does too.

  With both sources closed, a sweep of every other hole-consuming path found no
  remaining user-reachable producer: destructuring, rest, spread, `at`, `slice`,
  `concat`, `flat`, `shift`, `splice`, `find`/`findLast`, `map`, `filter`,
  `indexOf`, `includes`, `join`, `sort`, `reduce`, `forEach`, `for-of`, `in`,
  `Object.values`/`entries`/`assign`, object spread, `JSON.stringify`,
  `Array.from`, `structuredClone`, `with`/`toSorted`/`toReversed`/`toSpliced`,
  `copyWithin`, `reverse`, `fill`, and the `entries`/`values` iterators all
  already translate.

  **Consumer ladders**, kept as defence in depth against the next producer and
  pinned by unit tests rather than a fixture, since nothing user-facing reaches
  them any more:

  - `crates/perry-runtime/src/builtins/arithmetic.rs` — `classify_value_typeof`
    had no hole arm, so `typeof` answered `"number"`.
  - `crates/perry-runtime/src/value/to_string.rs` — `js_jsvalue_to_string`
    rendered a hole `"NaN"`. `String(x)`, `` `${x}` `` and `x.toString()` all
    funnel through this one helper.

  **`console.table`, two independent mechanisms:**

  - Hole cells printed `NaN`, and a cell-only fix would still not have matched
    node: node derives the columns from the UNION of each row's OWN keys
    (`Object.keys(row)`), and a hole is not an own key. `console.table([[1, , 3]])`
    has columns `0` and `2` — the hole's whole column is absent. The header
    derivation now computes that union, and a slot a row does not own renders as
    an empty cell exactly like a short row's missing tail. Column ORDER matches
    node's too: its column map is keyed by index strings, which `Object.keys`
    returns in numeric order however the rows contributed them, so
    `[[1, , 3], [4, 5, 6]]` is `0 1 2` and not `0 2 1`.
  - `delete o.a; console.table(o)` printed `b | NaN`. `object_key_names`
    compacted the tombstoned key out of a `Vec` **without keeping each key's slot
    index**, and the fields were then read back by the compacted position — so
    every key after a deleted one read its predecessor's slot. `format_object_as_json`
    and both `console.rs` option decoders iterate `0..key_count` and `continue`
    on a non-string key for exactly this reason; `table.rs` was the outlier and
    now keeps the index. This also restored the nested-row case
    (`delete o.r1; console.table(o)` had collapsed to a single `Values` column).

  **A silent deopt from the same shape:** `param_type_guard.rs`'s `OP_MAP` /
  `OP_SET` arms walked `0..size` over raw slots. A tombstone read as a real
  entry matches no descriptor node, so a legitimate `Map<string, number>`
  parameter lost its specialized clone after any `.delete()` — and the live
  entries past `size` went unchecked. Both arms now walk `0..used`, skip
  tombstones, and require the walk to have seen exactly `size` live entries.
  `OP_ARRAY`/`OP_TUPLE` still *reject* a hole: there it is an element, here it
  is bookkeeping.

  Affected files:

  - `crates/perry-runtime/src/array/indexing.rs`
  - `crates/perry-runtime/src/array/push_pop.rs`
  - `crates/perry-runtime/src/builtins/arithmetic.rs`
  - `crates/perry-runtime/src/value/to_string.rs`
  - `crates/perry-runtime/src/builtins/table.rs`
  - `crates/perry-runtime/src/param_type_guard.rs`

  Validation: `test-files/test_gap_9462_hole_leak_family.ts`, byte-compared
  against node 26.5.1 — 61 of its 128 stdout lines diverge on unfixed
  `origin/main`, and none after. The guard's accept/deopt verdict is not
  observable from TypeScript (it only chooses which clone runs), so it is
  asserted directly in `param_type_guard.rs`
  (`a_tombstoned_entry_does_not_deopt_a_collection_parameter`: a clean Map
  accepts, a Map with a deleted key still accepts, and a lie in the entry past
  the old `size` bound still deopts — the last is what proves the live tail is
  really walked). `array/collection_tag_tests.rs` asserts the leak source on the
  helper itself, comparing against `js_array_length`'s live count rather than
  merely "not a hole". `perry-runtime --lib`: 2980 passed, 0 failed.
