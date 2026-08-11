### perf(codegen): a declared array type reaches the guarded element read, and `.length` stops refusing one

Round 7 of the `interp` campaign. Two changes in the same mechanism — what a
program is allowed to do with an array type it got from an *annotation* rather
than from an initializer that proved an array.

#### A. `e.vals[i]` / `p.toks[p.pos]` — a property read used directly as a receiver

#7854 taught `refine_type_from_init` to recover a receiver's declared property
type for a **local** (`const names = e.names` on `type Env = { names: string[] }`),
which is why `names[i]` is an inline element read today. It did nothing for the
same read used **directly as the receiver** — `e.vals[i]`, `p.toks[p.pos]` —
because the HIR types a `PropertyGet` off a UNION receiver as `Any`
(`perry-hir/src/analysis/value_types.rs`, the `Union` arm), so `static_type_of`
answers `Any` and `expr/index_get.rs` routes the read to the unknown-receiver
dispatcher `js_dyn_index_get`.

`declared_array_property_claim` answers the question for that shape, and
`index_get.rs` consumes it in exactly two places: it suppresses the
`recv_unknown` route, and it admits the receiver to the array arm. The tier this
unlocks is `lower_guarded_array_index_get`, which re-checks `GC_TYPE_ARRAY`, the
forwarding flag, per-array descriptors, the prototype latch and the bounds **on
the receiver itself**, and routes every failure to
`js_typed_feedback_array_index_get_fallback_boxed`. So a violated claim costs a
predicted branch and returns the same answer — the deal #7854 already records
for element reads, and the same guard #6132 relies on to make a
typed-array-valued member receiver safe on this path.

The claim is restricted to a **non-string, non-symbol key**, and that
restriction is load-bearing rather than tidy. The array arm's static-string-key
route is `js_array_get_index_or_string` → `array_get_property_by_key` →
`js_object_get_field_by_name`, which has no string-receiver index arm and
answers `undefined` for `s["0"]` where JS answers the character — a *pre-existing*
wrong answer on `main`, reachable today through a plain non-union declared
receiver, filed as **#7891** with a minimal repro (not checked in as a gap test:
it would be red by construction, and `test-parity/gap_snapshot.json` is generated
on Linux and must not be hand-edited). The numeric
route has no such hole (`js_array_get_f64` classifies the receiver through
`clean_arr_ptr` / `array_object_receiver`). So the two key routes of the same arm
have different receiver-validation strength, and only the numeric one is
claim-safe. A string or symbol key keeps exactly the path it has today.

Measured share on `gc-handoff/apps/interp.ts` before the change (xctrace time
profile, `PERRY_DEBUG_SYMBOLS=1` build): `js_dyn_index_get` 5.0%,
`js_array_length` 4.6% — the latter reached from `js_dyn_index_get`'s and the
IC-miss handler's `.length` short-circuit.

#### B. `.length` no longer refuses a declared-only array local

#7854 recorded these locals in `FnCtx::declared_only_array_locals` and had the
inline `.length` arm refuse them. The reason was specific and correct at the
time: the arm's inline half was guarded, but its FALLBACK was
`js_value_length_f64`, which answered **0** for every value that carries no
length where JS answers `undefined`, and continued instead of throwing for a
nullish receiver (#7853).

**#7862 replaced that fallback with `js_value_length_property_f64`** — ordinary
property semantics: `undefined` for a missing property, the real value for a
non-numeric one, normal object / function / native / proxy dispatch, and a
catchable `TypeError` for a nullish receiver. It did not lift the refusal that
existed only because of the old fallback. This lifts it, and deletes the set and
its classifier with it: a mode that no longer gates anything is not a decision
that has been made.

`declared_only_numeric_locals` (#7773) is untouched and stays — its consumer is
an arithmetic operator with no guarded fallback, which is a different situation.

The sabotage is pre-existing and now runs the inline arm instead of the generic
tower: `test-files/test_gap_7853_declared_array_length_runtime_value.ts` and
`test-files/test_gap_declared_field_type_refine_guarded.ts` feed a
`string[]`-declared local an array, a string, a number, an array-like object
with a numeric `length`, an array-like object with a *non-numeric* `length`, a
function, a typed array, `null` and `undefined`, through an alias, an interface
and a class, and require node-identical output on every row.

#### Coverage

`test-files/test_gap_7890_declared_array_receiver_element_read.ts` is new. #7854's
own test always routes through an intermediate local (`const items = e.items`), so
nothing covered a `PropertyGet` used **directly** as the receiver — which is
exactly what half A adds. The new file reads `e.items[i]` / `e.items.length`
through a `type` alias, an `interface`, a class, a nullable reassigned cursor and a
nested chain, handed an array, a string, a number, an array-like object with
numeric and non-numeric `length`, a typed array, a function, `null` and
`undefined`, plus negative / fractional / out-of-range indexes, a store through the
same shape, and static string keys (which this change deliberately leaves on the
generic path). Live rather than decorative: on that file the
new arm's `arr.fast` guarded-read blocks go **11 → 15** and its
`js_dyn_index_get` calls go **5 → 1**.

Writing it is what found #7891.
