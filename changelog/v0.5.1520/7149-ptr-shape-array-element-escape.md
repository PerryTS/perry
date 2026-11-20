`Ptr<Shape>` rule 2 disqualifies a local at any escape. #7034 §4 opened
`return`; this opens **`array/object element`** for the array half —
`rows.push(row)` and `for (const r of rows) r.field`.

`return` was easy because a return is a **terminator**: no use of the local can
follow it, so every access the pass licensed had already run while the object
was unaliased. An element escape is not. The object stays reachable through the
array for the rest of its life, so the containment region had to widen from
*one local* to *one local array and everything derived from it*, and the
array's own uses had to be bounded exactly as an object local's are.

## The rule (`collectors/ptr_shape_elements.rs`)

A region-local `A` is an **element-shape-proven array** of class `C` when all
of:

- **E1 provenance** — exactly one `Let { mutable: false, init: Array([]) }`,
  not boxed, not a module global. The literal must be **empty**: a non-empty
  one can carry elisions, whose slots read back as `undefined`.
- **E2 element provenance** — every write is `ArrayPush` of `new C(...)`,
  inline or via a local bound by one `Let { init: New { C } }` and pushed
  **exactly once**. No other mutator at all — no `pop`/`shift`/`splice`/
  `unshift`/`copyWithin`, no `IndexSet`, no `length` write. That is what makes
  `A` dense and monomorphic for its whole lifetime.
- **E3 array containment** — every other use is an E5-licensed element read, a
  `.length` read, or `return A` (#7034 §4's terminator exemption, unchanged).
  Call argument, closure capture, reassignment, container element, unrecognised
  array method: all still disqualify. So does **any element read the `Let` arm
  did not license** — `f(A[i])`, `A[i].m()`, `const r = A[0]`. A read cannot
  transition a shape, but the reference it hands out can be used to, and rule 2
  never walks a binding this pass did not seed. That is why direct
  `A[i].field` is not covered at all today (#7151).
- **E4 admissibility** — `C` passes the same `chain_admissible` gate rule 1
  applies to a `new C(...)` local, and the rule-5 module barrier is clear.
- **E5 in-bounds reads** — `A[i]` is licensed only where `i` is the induction
  variable of an enclosing `for (let i = 0; i < A.length; i++)`, written
  nowhere else in the region. **This conjunct is the whole difference between
  this pass and a wrong one.** Without it `A[i]` can be `undefined`, and a
  guard-free fixed-offset load masks a NaN-boxed `undefined` into a wild
  pointer. E3 admits no mutator that can shrink `A` and E2 makes it dense, so
  `0 <= i < A.length` at the read means `A[i]` is an own element of class `C`.

`for (const r of A)` desugars to exactly the E5 shape
(`lower/stmt_loops.rs::lazy_or_index_elem`), so the iterator form is covered by
the indexed proof rather than by a second one.

Then two halves in `ptr_shape.rs`: `A.push(row)` stops disqualifying `row`, and
`const r = A[i]` at a licensed site is rule-1 provenance of `new`-strength.

**Group integrity.** Every member of an element group — the pushed producers
plus the element-read locals — references objects the *other* members also
reach. One member failing rule 2 (`r.extra = 1`, a closure capture, an opaque
call) can transition the shape the others read guard-free, so the group is
all-or-nothing: `collect_shape_proven_ptr_locals` drops every member when any
one fails. No fixpoint is needed — dropping never admits a member.

**`numeric_fields` is not claimed** for group members. The numeric proof is an
exhaustive-reachable-store proof that containment makes possible *because no
alias exists*; a group has aliases by construction, and a sibling's
`r.score = "s"` (a declared field, so rule 2 permits it) downgrades the slot's
raw-f64 layout. Same stand-down as `proven_this.rs` and `ptr_shape_returns.rs`.
The shape proof alone still retires the whole guard diamond.

## What it buys, measured

On a build-then-consume kernel (40 000 records, produced with a local, then
read back both ways) the promoted function goes **0 → 3 selected, 3 consumed**,
and its emitted IR loses:

| symbol | base | after |
|---|---:|---:|
| `js_typed_feedback_class_field_get_guard` | 7 | **0** |
| `js_typed_feedback_record_fallback_call` | 7 | **0** |
| `js_object_get_field_by_name_f64` | 7 | **0** |
| all `js_*` calls | 72 | 51 |
| IR lines / blocks | 1426 / 127 | 1076 / 113 |

**Every other `js_*` call count is identical**, including all of
`js_shadow_slot_bind` (5), `js_write_barrier_root_nanbox` (5),
`js_write_barrier_slot` (1), `js_array_push_f64` (2), `js_gc_loop_safepoint`
(4) and the inline incremental-mark barrier sites (5). Nothing but guard
machinery went away.

## GC contract, verified in emitted IR

- The element locals get `js_shadow_slot_bind` in the entry block (slots 3 and
  4 of the probe). `TaPtr`'s callee-side no-bind shortcut is **not** copied —
  `GC_TYPE_OBJECT` is movable (#6990, #7019).
- Every access **re-derives** the raw pointer from that alloca:
  `load double, ptr %rN` → `and POINTER_MASK` → `inttoptr` → `gep +header` →
  `gep index` → `load`. The `for…of` local is reloaded 3× (3 field reads) and
  the indexed local 4× (4 access sites); nothing is cached across a safepoint.
- The store of the element into the bound slot is followed by the incremental
  mark-barrier check and `js_write_barrier_root_nanbox`.
- Write barriers on element stores are untouched: this pass changes no store
  lowering, and `js_array_push_f64` counts are identical between arms.
- The read side uses the `ptr_shape_get_number.plain` / `.coerce` pair — the
  2-instruction plain-finite check with a cold arm — because the group claims
  no numeric fields.

## What this does NOT reach, and the measurements that say so

**`batch.ts` is unchanged: 2 selected / 1 consumed, identical to `main`.** Both
of its element denials fail for reasons outside this rule:

- `buildRows`'s `const rows = []; …; return rows` never reaches the analysis —
  the **interprocedural deforestation pass** (`perry-transform/src/deforest`)
  has already rewritten it into a `__deforest_out` *parameter*, and a parameter
  array has no provenance. That transform fires on exactly the
  `const a = []; …push…; return a` producer shape this rule targets, which is a
  real coverage hole rather than an incidental one.
- `summarize`'s `byBucket` is passed as `rows.reduce(…)`'s seed — a call
  argument, so the array escapes (#7034 §1 territory).

**Dependency JS gets essentially nothing.** #7139 reported that ~103 candidates
its CJS barrier exemption freed were "immediately re-denied by rule 2", and
this position was picked on the assumption that those were element escapes.
They are not. Over **180** real `__esModule` CJS modules from
`real-apps/scriptc/node_modules`, compiled by a #7139-only arm and a
combined (#7139 + this change) arm — both 180/180 — the 746 `Ptr<Shape>`
candidates deny as:

| bucket | count |
|---|---:|
| rule 1 — allocation never bound to a local | **506** |
| rule 5 — module barrier still armed | 99 |
| rule 2 — bare reference | 130 |
| rule 2 — call argument | 5 |
| rule 2 — **array element** | **1** |
| rule 2 — closure capture / undeclared property | 1 / 1 |

Both arms are identical on every line. The rule-2 bare references are all
Perry's own `__cjs_module` wrapper local, and the 506 rule-1 denials break down
as constructor argument 182, statement 162, call argument 84, return 64, array
element 8, initializer 6. **The wall in dependency JS is rule 1 (allocations
never bound to a local), not containment.**

## Review findings (CodeRabbit, PR #7149)

Each reviewer reproducer was added as a test **before** any fix, so the finding
had to prove itself red first.

- **🔴 group integrity did not drop a member's ALIASES.** Genuine, red on HEAD.
  The insert loop gives every alias of a promoted root the same fact, and the
  removal loop only removed the ids `group_members()` reports — so
  `const a = row` kept a guard-free proof of a shape a sibling had just
  transitioned. Fixed: the removal now takes the alias closure. Sabotage case
  `ALIAS_CLOSURE`.
- **🟠 a tracked array pushed into another array kept its facts.** Genuine, red
  on HEAD. `PushValue::Other` disqualified the OUTER array, but the arm skips
  `walk_expr` for a `LocalGet` value so the INNER one was never disqualified,
  leaving it reachable through `outer[0][0] = …` — an `IndexSet` on an
  `IndexGet` that neither walk tracks. Fixed with the reviewer's one-arm patch.
  Sabotage case `18_nested_array_push`.
- **🔴 a property store through `A[i]` was admitted for any property.** The
  hazard was real; its mechanism (`element_access_is_admissible`) had already
  been deleted in the same commit the review was posted against, when the
  unlicensed-element-read hole was closed. Both reproducers (`PropertySet` and
  `PropertyUpdate`) are **green on HEAD** and kept as permanent regression
  tests — sabotage cases `16_element_escape` and `19_element_prop_store` show
  which guard now carries them.
- **🟡 `is_empty` covered only `arrays`.** The other three maps are consistent
  with it by construction, but nothing enforced that and `is_empty()` gates
  every consumer. Assertion added in both directions.
- **🟠 "make `repsel-census` a required status check"** — declined, with the
  rationale on the thread: branch protection is admin-only and the deferral is
  deliberate project policy (a gate that has never been green blocks every open
  PR the moment it is promoted). Tracked as an open follow-through.
- Nitpicks taken: `facts_for` now receives the test's own classes (a mutated
  class reaching only `chain_admissible` while dispatch facts came from a
  pristine one is the vacuous-pass shape); the accessor fixture's getter no
  longer shadows a declared field (it could have passed on field/method
  ambiguity); `element_read_seeds` reuses this module's walker instead of a
  second copy of the traversal; and the array-ALIAS path — which the whole
  `for…of` read form runs through — now has a direct unit test.

## Validation

- `cargo test -p perry-codegen --lib`: 453 passed (26 new).
- **Sabotage matrix, 21 conjuncts, each with a disjoint red set** — every guard
  deleted in turn, the suite re-run, and the failing tests recorded: push
  exemption, in-bounds read, both GC rooting obligations, group integrity,
  single-push, empty-literal seed, `const` array binding, shrinking mutators,
  indexed store, class agreement, index write count, bare array reference,
  closure capture, `.length` receiver, unlicensed element read, unlicensed
  element binding, nested-array push, element property store, group integrity,
  alias closure. Control green in all 22 runs. Three weaknesses it caught
  and fixed: `a_local_pushed_into_two_arrays_is_not_exempt` passed on `HashMap`
  iteration order (now asserts the facts directly), the closure-capture test
  denied through the body walk rather than the capture list, and
  `unbounded_element_read_is_not_provenance` asserted only that the READ was
  denied, not that the array was voided — which is the assertion that catches
  the element-escape hole above.
- Census: new liveness fixture `fixture_ptr_shape_elements` with floors
  `ptr-shape 3 / ptr-shape-consumed 3`; no existing floor moved; the
  `PERRY_PTR_SHAPE_LOCALS=0` sabotage step in `repsel-census` now asserts the
  element fixture goes to zero too — without that, the whole analysis could
  stop issuing facts and every counter in the job would be unchanged.
- New `test_gap_repsel_ptr_shape_elements.ts`, registered in
  `test-parity/gc_repsel_corpus.txt`, byte-exact against Node 26.5.1: both read
  forms, 200 allocations *between* two field reads of the same element local,
  NaN/±Infinity/-0 written through one group member and read through another,
  and four arrays that must not be proven.
- `gc_repsel_matrix.sh --arms all --pressure 8`: **PASS=447 UNVER=119 XFAIL=1
  FAIL=0** over 567 cells, `requires=move` arms live. The new gap file is
  **PASS in all 21 arm columns with zero UNVER** — every arm, evacuating ones
  included, was live on it, so those greens are not the inert-arm kind
  (#6942/#6946/#6950).
- Emitted objects are **byte-identical** between arms on `batch.ts`,
  `02_loop_overhead`, `04_array_read`, `07_object_create`, `12_binary_trees`
  and `15_mandelbrot` — nothing changed where nothing promotes. (#7131 landed,
  so object hashing is a valid instrument.)

## Follow-ups filed

- **#7150** — deforestation rewrites `const a = []; …push…; return a` into a
  `__deforest_out` parameter before this analysis runs, which is why `batch.ts`
  is unchanged. The two passes are working against each other.
- **#7151** — the four element-read forms this does not cover (direct
  `A[i].field`, callback parameters, `let` bindings, non-empty literals and
  shape-preserving mutators), with the measurement each needs first.
- **#7152** — the dependency-JS wall is rule 1 (506 of 746 candidates are
  allocations never bound to a local), not containment. `call argument`
  (#7034 §1) should not be scheduled on the assumption that it unlocks
  dependency JS either: it is 5 of 746.
- **#7153** — pre-existing: reading a field of an out-of-bounds array element
  returns `undefined` instead of throwing `TypeError`. Found while writing the
  gap test; red on `main` and with the knob off.
