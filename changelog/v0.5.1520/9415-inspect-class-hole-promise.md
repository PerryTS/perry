**`console.log` / `util.inspect` no longer decode a class as an integer, a
sparse-array hole as `NaN`, or a settled promise as pending.**

```js
console.log(class Klass {})   // was "1"                  now "[class Klass]"
console.log(new Array(3))     // was "[ NaN, NaN, NaN ]"  now "[ <3 empty items> ]"
console.log(Promise.resolve(1)) // was "Promise { <pending> }" now "Promise { 1 }"
```

Three separate defects with one shape: a ladder classifies a NaN-boxed value
by tag, has no arm for the case in hand, and lets the bits fall through to
"must be a regular number".

A class value is an INT32-tagged NaN box carrying the class id, so every
`else if v.is_int32()` arm printed `as_int32()` — the raw id. `INT32_TAG | 2`
and a ClassRef with `class_id == 2` are bit-identical; `class_ref_id`'s
registry probe is the only thing separating them, exactly as
`symbol/iterator.rs` documents for `for…of`. Class ids are small and
sequential, so a program with N classes leaves the integers `1..=N`
genuinely undecidable at the display ladder; perry now answers "class" for
those, because a class id leaking into output is never right. The probe stays
inside the `is_int32()` arm, where a value is already being turned into a
heap `String`, so ordinary numbers — plain f64 doubles — never pay for it.

`TAG_HOLE`'s bit pattern *is* a NaN, which is why a hole printed as `NaN`
rather than crashing. Runs of holes now collapse to Node's `<N empty items>`,
and the single-line/multi-line decision counts the entries Node prints
instead of the array's `length` — `new Array(7)` is seven slots but one
entry, so it stays on one line. The same sentinel is why a tombstoned
`Map`/`Set` inspected wrongly: `js_set_delete` writes `TAG_HOLE` over the
slot and decrements `size` without touching `used`, so walking `0..size` both
rendered the tombstone and stopped short of the live tail
(`new Set([1,2,3])` after `delete(1)` printed `Set(2) { NaN, 2 }`). Both
walks are now bounded by `used` and skip holes, like the collection iterator
objects already did.

The promise arm was a hard-coded `"Promise { <pending> }"` string; it now
reads the state byte, and `format_jsvalue_for_json` gained the promise arm it
never had, so a promise-valued field says `Promise { 1 }` instead of
`[object Object]`.

All three renderings live in one `builtins/formatting/value_repr.rs` shared
by the ladders in `console.rs` and `formatting.rs`, because a fix applied to
`console.log` and not `console.error`, or to `format_jsvalue` and not to
`format_jsvalue_for_json` (which renders the same array once it is an object
field), is a half-fix that reads as a working one.

One consequence had to be paid for: `util.isDeepStrictEqual` compares the
formatted rendering of two non-pointer operands, and two DISTINCT classes that
share a name now render identically where their class ids used to differ. A
class reference is therefore compared by identity in that tail — after
`js_jsvalue_equals` has already settled the equal case, so an ordinary integer
is unaffected.

The bit-identity collision turns out not to be observable through the display
ladders at all: a JS number is a plain f64 double and never reaches the INT32
arm. Measured with class ids 1 and 2 live, `console.log(1)`, `console.log(2)`,
`[9].length`, `"A".charCodeAt(0)` and `3 | 0` all still print integers. The
registry probe is the second line of defence, not the only one.

`test-files/test_gap_9415_inspect_class_hole_promise.ts` is byte-compared
against node. Built from unfixed `origin/main` the same fixture diverges on
34 of its stdout lines and on all 4 of its stderr lines.
