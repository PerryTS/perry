Fixed two independent conformance bugs that both live one receiver-kind question
away from a buffer fast path (#8149, #8139).

**#8149 — `DataView` and raw `ArrayBuffer` were byte-indexable.** Perry backs
FOUR distinct JS types with the same `BufferHeader` and the same
`BUFFER_REGISTRY` entry: a node `Buffer`, a `Uint8Array`, an
`ArrayBuffer`/`SharedArrayBuffer`, and a `DataView`. Only the first two are
integer-indexed exotic objects, but every consumer that triaged a receiver as
"registered buffer ⇒ byte indexable" served all four. `dv[0]` answered the byte
where node answers `undefined`; `dv.length` / `ab.length` answered the byte
count where node answers `undefined`; `0 in dv` and
`hasOwnProperty.call(dv,"0")` answered `true` where node answers `false`; and
`dv[0] = 7` OVERWROTE a byte where node creates an ordinary own property and
leaves the byte at 0.

The `Object.keys` half was a memory-safety bug, not merely a wrong answer. The
enumeration paths had no registered-buffer arm at all and fell through to the
generic walk, which reads a `BufferHeader`'s PAYLOAD as
`ObjectHeader.keys_array` and then calls `js_array_length` on it. That answered
`[]` whenever those bytes happened to be zero — which is all
`Object.keys(Buffer.from([1,2,3]))` looked like — and SIGBUS'd (exit 138,
`js_array_length` ← `js_object_keys`) when they did not:

```ts
const ab = new ArrayBuffer(8);
const D = new DataView(ab);
const b = Buffer.from([1, 2, 3]);   // required: it changes the allocation layout
console.log(JSON.stringify(Object.keys(D)));
```

`Object.entries(b)` and `Object.values(b)` reached the same walk. Also fixed
along the same arm: `Object.getOwnPropertyNames(b)`, `{...b}` /
`Object.assign({}, b)` (both were `{}`), and `JSON.stringify(dv)` /
`JSON.stringify(ab)`, which emitted `{"type":"Buffer","data":[…]}` — a shape
node never produces for those receivers, and one that leaks the backing bytes.

New `buffer::exotic_view` owns the discrimination
(`is_non_indexed_buffer_view`, `is_byte_indexed_buffer`,
`canonical_index_key`), and every call site asks it ABOVE the arm it guards,
never below: the byte arm answers unconditionally, so a re-check placed after
it is dead code. That is the ordering #8090 / #8109 / #8119 / #8120 / #8124 /
#8140 / #8141 / #8148 / #8173 each had to restore. Both buffer backings are
covered by construction — `is_registered_buffer` is a side-table membership
test, not an address-range or GC-header probe, so an EXTERNAL buffer (no
`GcHeader` at all; see `array/header.rs`'s `array_receiver_gc_tag` doc, #8142)
is classified by exactly the same lookups as an arena-backed one.

**#8139 — `toLocaleString` rendered `[object Array]` for every array.** A
different cause, one level further out. The HIR folds the zero-arg
`x.toLocaleString()` on ANY receiver to `Expr::DateToLocaleString`
(`lower/expr_call/url_date_instance.rs`), so `js_object_default_to_locale_string`
— not the method-dispatch tower — is where every such call is answered. It had
arms for number / `Date` / `Temporal` / `BigInt` / primitives and then fell
through to `Object.prototype.toLocaleString`'s `Invoke(O, "toString")`. The
plain-array row is the tell that this is NOT a reroute bug in the array
helpers: `js_array_to_locale_string` was always correct, and the
argument-bearing spelling (`arr.toLocaleString("en-US")`, which does not fold)
has always reached it — the two spellings simply disagreed. Each new arm
delegates to the exact helper the tower would have used; `js_native_call_method`
is deliberately not used, because its `common_methods` `toLocaleString` arm
calls straight back into this function. `dispatch_buffer_method`'s
`toLocaleString` additionally declines a `is_uint8array_buffer`-marked receiver
so it reaches `uint8_join`: `Buffer.prototype.toLocaleString` is an OWN
override, and a plain `Uint8Array` inherits the `%TypedArray%` join
(`new Uint8Array([3,1,2]).toLocaleString()` is `"3,1,2"`, not three raw bytes).

Verified byte-for-byte against node `26.5.1` (the `.node-version` pin) across 7
probe programs / 96 rows. 22 tests in `buffer/exotic_view_tests.rs` and
`object/native_call_method/to_locale_string_tests.rs`, all asserting OBSERVED
VALUES, never predicates: a `DataView`'s bytes are zero-filled, so a probe
asking "is `dv[0]` falsy?" passes under the bug, and the typed-array
`toLocaleString` cases assert the digit GROUPING (`"1,234,567,2"`) because that
is the only thing separating a correct implementation from a `join()`
delegation. The store-side tests assert BOTH halves — the expando exists AND
the byte is still zero — since a fix doing both would pass either alone. Six
sabotage arms were run and reverted: predicate always false (10 subject tests
fail, 6 controls pass), predicate over-reaching to every registered buffer (8
controls fail), enumeration arm deleted (exactly the 3 enumeration tests fail),
`toLocaleString` arms deleted (3 of 5 fail), the `!is_uint8array_buffer` gate
removed (the `Uint8Array` test fails), and the typed-array arm delegating to
`join()` (the grouping test fails). One of my own measurements was vacuous and
was replaced: an assertion read `Object.values(buffer)` through the string-key
helper, which would have passed either way.

`cargo test -p perry-runtime --lib`: 2467 passed, 0 failed, 4 ignored.

Deliberately out of scope, each with its reason recorded in the code:
`for…in` over a `Buffer` (node also enumerates `Buffer.prototype`'s ~100
enumerable methods); `new Uint8Array([3,1,2]).toString()` (#8139 part 2 — the
gate is one line, but `is_uint8array_buffer` is not a real `Buffer` brand:
`KeyObject.export()` marks its Buffer result so `instanceof Uint8Array` holds,
and unlike `toLocaleString` the pre-state here is a widely-relied-on decode);
own expandos inside `JSON.stringify(dv)`; and `dv[-1] = 1`, whose store perry
drops for a `Buffer` as well.
