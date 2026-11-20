### perf(codegen,runtime): boxed class-field stores get the inline precheck, the arguments-object registry gets an emptiness latch, and a property read into an untyped local recovers the receiver's declared type

Round 4 of the `interp` campaign (3.96 s → 1.893 → 1.499 → 1.237 → **this**). Three
independent changes, each measured on its own; plus one refutation that closes the
lever the previous round's handoff was built around.

#### A. The strict class-field SET arm now emits the inline precheck for BOXED fields

`expr/property_set.rs` gated `emit_class_field_inline_precheck` on
`requires_raw_f64`, so every store into a declared field that is *not* a `number` —
a `string`, a class type, a union: most fields of most objects — paid an
unconditional cross-crate `js_typed_feedback_class_field_set_guard` call. That
includes the synthesized `__AnonShape_*_constructor` every closed-shape object
literal runs, which is why `js_typed_feedback_class_field_set_guard` (2.9%) plus
`typed_feedback::guards::class_field_fast_contract` (2.1%) sat near the top of
`interp`'s profile: `{ kind: "bin", op, left, right }` is four stores, three of them
boxed.

The stated reason for the gate — "its setter-in-chain handling and write barrier
aren't reproduced inline" — was already answered by
`try_lower_sloppy_class_field_boxed_store`, which has taken the boxed inline
precheck since #7288. The write barrier, layout note and string demote come from
`emit_jsvalue_slot_store_pointer_tested` (which the shared fast block calls, with
the identical value-side predicates), not from the guard; a setter anywhere in the
chain is refused upstream by `class_field_global_index`'s `accessor_in_chain`. What
the precheck proves is a strict subset of the runtime's `class_field_fast_contract`,
so on a hit the guard call would have answered "fast" too — this removes a call, it
never changes which store happens. Every miss still lands on the guardcall block and
the unchanged strict fallback.

Verified live rather than assumed: `interp.ts` goes from 3 to 44 emitted
`PERRY_CLASS_FIELD_INLINE_GUARD_DISABLED` gate loads (41 new prechecks) with the
same 43 guard-call sites, now on the miss arm.

#### B. `is_arguments_object` gets the #7474/#7469 emptiness latch

`is_arguments_object` is a *probe*, called from the by-name property-get tail,
`Array.prototype.push`, the array and `Symbol.iterator` iterator entries,
`Array.from`/`concat`, and class construction. In a program that never writes the
identifier `arguments` it was still 2.8% of `interp`: a thread-local resolution
(Darwin has no local-exec TLS, so that is a real `_tlv_get_addr` call — 7.1% of the
same profile), a `RefCell` borrow, and a pointer hash, per call, to prove the
absence of a feature the source does not contain.

`ARGUMENTS_OBJECTS_EVER_USED` is a process-global `AtomicBool` latched by the one
and only registry insert, checked before the thread-local — the
`EXTERNAL_BUFFERS_NONEMPTY` / `SET_REGISTRY_EVER_USED` idiom verbatim. Global rather
than per-thread on purpose (a `thread_local!` flag would cost the very TLS call this
removes); being global only makes it conservative.

`object/arguments_latch_tests.rs` pins the subject, not the answer:
`latch_off_is_what_makes_the_probe_cheap` registers a real arguments object, forces
the latch back off, and requires the probe to answer `false` — deliberately the
wrong answer, and the only way to show the short-circuit is the arm being taken
rather than dead code in front of a registry that would have answered anyway. Delete
the early-out and that test goes red. `creating_an_arguments_object_arms_the_latch`
pins the other half (a second insert site added without arming would be a silent
wrong answer).

#### C. A property read into an unannotated local recovers the receiver's declared type

`const names = e.names` left `names` at `Any`, so `names[i]` lowered to a
`js_dyn_index_get` call (4.3% of `interp`) whose own miss path calls
`js_array_length` (2.9%), and `names[i] === name` lowered to the fully dynamic
`js_eq` instead of an inline string compare. `refine_type_from_init`'s `PropertyGet`
arm resolves the receiver with `receiver_class_name`, which answers `None` for a
reassigned local and for a union, and then looks only in `ctx.classes` — so a
chain-walking cursor (`let e: Env | null = env; … e = e.parent`) over a
`type Env = { … }` alias resolved to nothing three times over. That is the
`type`-vs-`interface` asymmetry #655 fixed for `static_type_of` but not here, plus
the nullish-union and reassignment gaps.

`declared_property_type_from_annotation` resolves through the same
class / interface / object-alias tables `static_type_of` already consults, after
stripping `null`/`undefined` from the receiver's union (a read that returns at all
had a non-nullish receiver — reading through `null` throws). It infers exactly the
type a hand-written `const names: string[] = e.names` would have installed.

**It is a claim, not a proof, and one consumer could not take one.** Element reads
and stores tolerate a violated claim — both re-check `GC_TYPE_ARRAY` and fall back.
`.length` does not: its inline arm is guarded but its fallback,
`js_value_length_f64`, answers **0** for every value that carries no length where JS
answers `undefined` (and where a nullish receiver must throw) — a pre-existing
degradation the runtime documents in place, and one that is therefore already
reachable on `main` through a hand-written annotation (filed as **#7853**). So
`.length` must not be handed a fresh claim: `refined_array_type_is_declared_only` records these ids in
`FnCtx::declared_only_array_locals` (the `declared_only_numeric_locals` mechanism
from #7773) and the `.length` arm in `expr/property_get.rs` refuses them, leaving
them on exactly the generic path the unrefined `Any` local takes today.

`test-files/test_gap_declared_field_type_refine_guarded.ts` is the sabotage test:
the same `items: string[]` declaration is handed arrays, strings, plain objects
aping arrays, numbers, `null` and `undefined`, through an alias, an interface and a
class, through a nullable reassigned cursor and a nested read chain, and every row
must match node byte for byte. **It was written before the guard and it failed** —
four rows read `len=0` where node says `undefined` or throws — which is how the
`.length` hazard above was found rather than shipped.

#### Measured

Quiet M1 mini, best-of-5, exit-checked, outputs byte-compared against
`node --experimental-strip-types`. See the PR body for the full 19-program table.

#### Refuted: shape narrowing after a discriminant test

`PROFILE-interp-round3.md` identified `evalNode`'s surviving property-read diamonds
(27.2% of the program) and proposed narrowing `n` to its matching union member
inside `if (n.kind === "bin")` so the reads become class-keyed slot loads. **The
ceiling was measured before building it, and it is ~5%, not ~20%.**

Three source-level arms of `interp.ts`, identical in every other respect, built from
one compiler: object literals (the original), the same program with each union
member as a real `class` (so allocation, not typing, is isolated), and that program
with hand-written narrowing casts in every `evalNode` arm. The narrowed arm converts
**19 of 31** generic property diamonds into guarded class-field inline reads — and
is 1.226 → 1.162 s against its own control, 5.2%. The reason is structural: the
class-field guarded read is only about a third cheaper than the polymorphic-IC read,
because the cost is the *guard*, not the lookup, and narrowing replaces one guarded
diamond with another. A large win there needs the check hoisted out of the branch
(one shape test, N unguarded slot loads), which is loop-versioning applied to a
discriminant arm — a much bigger build than the handoff assumed, for a lever whose
cheap form is now measured and closed.
