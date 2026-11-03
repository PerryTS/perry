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
