### fix(codegen): keep a string or symbol key off #7890's declared-array claim, and cover the shape #7890 added

Two follow-ups to #7890, both found by writing the coverage #7890 was missing.

#### A. A string/symbol key must not ride the claim (#7891)

#7890 lets a property read whose receiver's *declared* property type is an array
(`e.vals[i]`, `p.toks[p.pos]`) reach `expr/index_get.rs`'s array arm. That is a
CLAIM, not a proof, and it was admitted on the grounds that the array arm
re-checks `GC_TYPE_ARRAY` on the receiver and falls back.

That is true of the arm **as a whole** and false of one route inside it. The two
key routes have different receiver-validation strength:

* **numeric** — `js_array_get_f64`, which classifies the receiver through
  `clean_arr_ptr` / `array_object_receiver` and answers correctly for a string,
  an array-like object, a typed array or a number.
* **static string / symbol** — `js_array_get_index_or_string` →
  `array_get_property_by_key` → `js_object_get_field_by_name`, which has **no
  string-receiver index arm** and answers `undefined` for `s["0"]` where JS
  answers the character.

So only the numeric route is claim-safe. The claim now requires a non-string,
non-symbol key; a string or symbol key keeps exactly the generic path it had
before #7890. `interp`'s and `iso_miss`'s reads are all numeric, so the measured
result is unchanged — every one of the 19 corpus binaries is byte-identical to
the ones timed for #7890.

The `undefined` answer itself is **pre-existing on `main`** and reachable without
any of this, through a plain non-union declared receiver:

```ts
type Bag = { items: string[] };
function mk(v: any): Bag { return { items: v }; }
function viaDeclared(b: Bag): string { return "" + b.items["0"] + "/" + b.items[0]; }
const s: any = "ss";
console.log(viaDeclared(mk(s)));                 // node: s/s   perry: undefined/s
const direct: any = "ss";
console.log("" + direct["0"] + "/" + direct[0]); // node: s/s   perry: s/s
```

The same read on a bare `any` is correct, which is the tell: the wrong answer is
selected by the ANNOTATION, not by the value. Tracked as **#7891**; not checked in
as a gap test, because it would be red by construction and
`test-parity/gap_snapshot.json` is generated on Linux and must not be hand-edited.

#### B. Coverage for the shape #7890 actually added

`test-files/test_gap_7890_declared_array_receiver_element_read.ts`. #7854's own
test always routes through an intermediate local (`const items = e.items`), so
nothing covered a `PropertyGet` used **directly** as the receiver — which is
exactly what #7890 added. The new file reads `e.items[i]` / `e.items.length`
through a `type` alias, an `interface`, a class, a nullable reassigned cursor and
a nested chain, handed an array, a string, a number, an array-like object with
numeric and with non-numeric `length`, a typed array, a function, `null` and
`undefined`, plus negative / fractional / out-of-range indexes, a store through
the same shape, and static string keys (which A leaves on the generic path).
Byte-identical to node on every row.

Live rather than decorative: on that file the guarded-read `arr.fast` blocks go
**11 → 15** and the `js_dyn_index_get` calls go **5 → 1**.

Writing it is what found #7891.
