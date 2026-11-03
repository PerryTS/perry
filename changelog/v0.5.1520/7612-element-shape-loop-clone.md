### Repsel: the element-shape versioned loop clone — first consumer of the element-shape invariant (#7480 / #5093)

#7496 landed the per-array homogeneous element-shape invariant with no
consumer, on purpose. This is the consumer.

`for (let j = 0; j < n; j++) sum += keep[j].v` now gets a specialized clone
behind a preheader guard on "this array holds the element-shape invariant at
class `C`". Inside the clone the element read is a bare
`gep + load` off a preheader-cached elements base, and the field read is a
bare raw-f64 slot load. The existing generic body survives unchanged as the
cold arm, so nothing that fails the guard changes at all.

**Measured on the #7480 access shape** — 200k-element array, 50 sweeps,
in-program timing, best-of-9 with the two arms interleaved so machine drift
hits both equally, checksums equal:

| kernel | before | after | node | speedup |
|---|--:|--:|--:|--:|
| `keep: Node[]`, `sum += keep[j].v` | 41 ms | **13 ms** | 13 ms | **3.15×** — now at parity with node (was 3.15× behind) |
| `keep: {v,w}[]` (object literal) | 86 ms | 86 ms | 10 ms | 1.00× — deliberately out of scope, see below |

**Size** (#7566's discipline, both traps avoided — runtime trip counts so
nothing unrolls, arrays escaping through `console.log` so nothing is
scalar-replaced), measured on the module object file rather than the binary so
the runtime archive does not blur it:

| probe | base | this | delta |
|---|--:|--:|--:|
| 40 loops, **none** qualifying | 83,000 B | 83,000 B | **+0 B, object bytes byte-identical** |
| 40 loops, all qualifying | 93,904 B | 108,872 B | +14,968 B (+15.9%, ~374 B per cloned loop) |

The runtime archive grows 232 B for the single `keepalive-anchors` static.

## Mid-loop revocation: which mechanism, and its failure mode

A store inside the loop body, or inside anything the body calls, can revoke
the invariant mid-iteration, and a specialized body reading a revoked array is
a **miscompile**, not a slow path. Of the three options — restrict the body,
re-check per back-edge, guard-at-entry plus deopt — this ships the first, and
enforces it twice:

1. **By shape (the matcher).** A single store-free `acc = <pure numeric>`
   statement over `arr[counter].field` reads, numeric locals, numeric literals
   and pure arithmetic / `Math`. No stores, calls, closures, `await`, or
   updates other than the counter's. A catch-all that silently admitted an
   unknown expression is the failure this walker refuses to have.
2. **By construction (the lowering).** After the fast clone is emitted, every
   one of its blocks is scanned for a GC-unsafe call. If any call survived,
   the deref block branches *unconditionally* to the slow clone and the fast
   blocks are left as unreachable code. A clone whose call-freeness is
   unproven is never entered.

Call-freeness is exactly the right property because **every** way to revoke
the invariant is a runtime call: an element store (`gc::layout_note_slot` →
`note_element_store`), a length change (`push`/`pop`/`shift`/`splice`/
`length = n`, all caught by the record's pinned `verified_len`), `delete`
(a `TAG_HOLE` store through the same funnel), `defineProperty` on the array
(`OBJ_FLAG_ARRAY_DESCRIPTORS`), prototype surgery
(`invalidate_all_element_shapes`) — and so is every allocation that could move
the array. Codegen's *inline* element store is the one path that can skip the
note, and only when the array is statically proven numeric and pointer-free,
which an element-shape array can never be.

**Failure mode: conservative, never unsound.** Anything that writes, calls, or
reads a field the analysis cannot type simply gets no clone. The residual risk
is a *silent loss of the optimization* — a lowering change that starts
emitting a call inside an admitted body would make the whole clone dead code
with nothing failing. `stmt/element_shape_loop_tests.rs` is the gate for that:
it asserts the fast blocks appear in the emitted IR **and** that the fast
clone contains no `call` at all.

A useful consequence of the same rule, verified in the emitted IR: under
`PERRY_GC_MOVING_LOOP_POLLS=1` the back-edge safepoint is itself a call, so
the scan fails and the deref block emits an *unconditional*
`br label %element_shape.loop.slow.preheader`. The clone stands down in exactly
the configuration where a mid-loop collection could move the array — with no
special case for it anywhere in the code.

**Sabotage, both directions.** Breaking the guard (every shape fact discarded,
per-element check never side-exits) turns the gap test into a **SIGBUS at the
`subclass:` case** — the `Array`-subclass `ObjectHeader` read as an
`ArrayHeader`, i.e. #7603's fault reproduced on demand. Breaking the clone
*selection* (matcher never fires) leaves the gap test byte-identical to node
while the IR census drops to the same zero the base compiler emits — so the
fallback is behaviour-neutral, and the census is measuring the clone rather
than something incidental:

| arm | clone blocks | `js_array_ensure_element_shape` calls | gap test |
|---|--:|--:|---|
| base (`main`) | 0 | 0 | identical to node |
| this | 1 of each | 1 | identical to node |
| sabotage: never-specialize | 0 | 0 | identical to node |
| sabotage: always-specialize | 1 of each | 1 | **SIGBUS, exit 138** |

## The guard tests the live header, and the brand is explicit

The preheader calls `js_array_ensure_element_shape` — #7496's own query
surface, which reads the array's current `GcHeader` bit and record and
self-heals when the record went stale. No inline reimplementation, so no
drift; #7501's lesson (a static declaration gets revoked at runtime) is
answered by construction.

Sequencing is load-bearing and is documented at the emitter. The
`GC_TYPE_ARRAY` brand test comes **first**, so the pointer handed to the
runtime is already branded — an `Array` subclass instance is a plain
`ObjectHeader` whose fields overlay `ArrayHeader`'s (#7573/#7603), and reading
one as an array is how #7603's SIGSEGV happened. The elements base pointer is
derived only **after** the guard call returns, from a fresh load of the
array's rooted slot: the call can allocate, and an allocation can move the
array.

## What the invariant does not prove, and what that costs

`element_class_of_bits` proves `POINTER_TAG`, a readable `GcHeader`,
`GC_TYPE_OBJECT`, `OBJECT_TYPE_REGULAR` and `class_id == C` for every element
in the verified prefix — exactly the predicates the element-read tier and the
front half of the field-read precheck spend per iteration, so the clone drops
them. It proves nothing about the per-*object* facts a raw-f64 slot load needs:
`keys_array` identity (a `delete elem.f` compacts the packed slots while
preserving `class_id`), `field_count`, the per-object descriptor flag, and the
typed-layout intact bit. Dropping those would be the miscompile, so the clone
keeps a residual per-element check — collapsed to one 4-byte load of the three
contiguous header bytes plus two more loads, AND-reduced into a single branch
that side-exits to the slow clone. Emitted fast body: **zero calls, one
branch, no volatile gate load** (the gate is hoisted, which is sound for
exactly the same reason the clone is).

Folding those facts into the invariant is the natural next slice, and it needs
an invalidation surface for `delete` / `defineProperty` / typed downgrade that
#7496 deliberately did not open. It should land the way #7496 did: invariant
first, matrix second, consumer third.

## Scope

Declared element types (`keep: Node[]`) only. #7480's own object-literal
kernel (`keep: {v,w}[]`) stays where it is, because `receiver_class_name`
returning `None` for an `Object`-typed element is also what makes the
number-context field-read helper decline — so a wider matcher would buy only a
block of dead fast-clone IR. Reaching it means teaching `static_type_of` /
`receiver_class_name` to type an `Object`-typed property read, which is
precisely the #6377 "more type visibility un-gates latent fast paths" change
and needs its own gap-suite A/B.

Element classes with a base class are declined: an inherited layout is not
described by the packed slot index alone, and a native base (`extends Array`)
is the #7573/#7603 hazard itself.

## Files

- `crates/perry-codegen/src/stmt/element_shape_loop.rs` — matcher + lowering,
  and the full revocation argument
- `crates/perry-codegen/src/expr/element_shape_guard.rs` — the preheader guard
  and the per-element residual check, with an anti-drift test that
  reconstructs the header mask from the individual runtime constants and
  sabotages each fact it must reject
- `crates/perry-codegen/src/expr/property_get/helpers.rs` — the consumption
  hook, next to #5093's class-field one
- `crates/perry-runtime/src/array/element_shape.rs` — exactly ONE
  `keepalive-anchors` static, for the one symbol codegen now emits a call to;
  the other four stay unanchored and dead-strippable
- `test-files/test_gap_repsel_element_shape_loop_clone.ts` — the hot shape plus
  every hazard: mid-loop store revocation (direct and through a call),
  revocation between two entries of the same loop, subclass receiver (both a
  `const` and a plainly-typed parameter), shape-mismatched and heterogeneous
  arrays, holes / sparse / `delete`, empty array, per-element typed-layout
  downgrade, deleted field, own accessor, frozen element, prototype surgery,
  every length mutation, and a bound past the array's length. Named into the
  `test_gap_repsel*` glob so the GC root-dominance corpus picks it up
  permanently.

## Verification

- Gap test byte-identical to node, exit 0.
- Gap-suite family A/B (`array` / `object` / `class` / `repsel` / `new` /
  `prop`, 71 tests) against a same-session `main` build: **verdict sets
  identical** — 70 PASS / 1 FAIL on both arms, the one failure
  (`test_gap_prop_plan_cache_invalidation`) pre-existing and present on both.
  This is #6377's gate, and it is the one that matters for a proof PR.
- GC root-dominance, curated corpus (129/129 sources compiled, 149 modules),
  **both gated modes**: `--moving-only` 0 violations with **40/40 seeded
  violations caught**, and `--unrooted-allocas --moving-only` 0 violations over
  7,860 GC-capable allocas. The allowlist is empty and stays empty.
- `cargo test -p perry-runtime --no-fail-fast` 1880 passed / 0 failed;
  `cargo test -p perry-codegen --lib` 685 passed / 0 failed (7 new).
- `cargo fmt --all --check`, file-size cap, addr-class ratchet, GC store-site
  inventory, workspace-architecture policy: all clean.
