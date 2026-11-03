### Fixed

**repsel: the element-shape versioned loop clone bus-errored on any array that
had grown outside the reading scope (#7480 / #7612).**

`js_array_grow` does not grow in place: it allocates the larger array
elsewhere, copies, and leaves a **forwarding stub** at the old address whose
first payload word (`length`‖`capacity`) is overwritten with the new head —
that overwrite *is* the chain `clean_arr_ptr` follows. Every runtime entry
point resolves it, so a binding holding a stale head still behaves correctly
through the runtime, and only bindings the growing code itself wrote through
get re-pointed. A callee that grows a caller's array, or a function that grows
a local and returns it, leaves a stale head behind — repsel 4a.2's canonical
case (#6904), which is why `js_array_refresh_local_head` exists.

#7612's preheader did not use it. It derived both the bound check and the
elements base from the raw pointer, and on a stub those two facts fail in the
worst possible combination: `length` reads the low 32 bits of a heap pointer,
so the "verified prefix covers the whole index range" test **passes**, while
the elements base still addresses the pre-growth buffer. The guard call one
block earlier resolved the chain internally and answered truthfully about the
live array, so the class-id test passed too. The clone then read correct
elements up to the old capacity and ran off the end of the block after it —
masked, dereferenced at `-8`, `SIGBUS`. With `MIN_ARRAY_CAPACITY == 16` that is
a bug with a threshold: right at 16 elements, bus error at 17.

```ts
function build(n: number): Node[] {
  const out: Node[] = [];
  for (let i = 0; i < n; i++) out.push(new Node(i, i * 2));
  return out;                                    // grew 16 -> 32, stub returned
}
function sweep(keep: Node[], n: number): number {
  let sum = 0;
  for (let j = 0; j < n; j++) sum += keep[j].v;  // bound is a LocalGet
  return sum;
}
const keep = build(17);                          // 17, not 16
sweep(keep, keep.length);                        // exit 138 before this fix
```

**Two ingredients, and the second is the one that makes this hard to
reproduce.** (1) the array must have grown past `MIN_ARRAY_CAPACITY` (16) in a
callee that returned it — minimal N is 17; and (2) the clone must actually be
emitted, which requires a loop bound that is `Expr::Integer` or
`Expr::LocalGet`. An inline `arr.length` bound is an `Expr::PropertyGet`, which
`match_element_shape_versioned_loop` rejects, so `for (let j = 0; j <
keep.length; j++)` emits no clone and **cannot** fault; hoisting the identical
bound into a local flips it. Nothing else matters: `--release` and
`--profile perry-dev` fault identically, auto-optimize is irrelevant, no
run-time knob is involved, and the read does not need to cross a function
boundary (a module-scope loop with a hoisted bound faults the same way).

Fixed with a new `element_shape.loop.preheader.repair` block between the brand
test and the guard call: follow the chain once with
`js_array_refresh_local_head`, write the live head back into the binding, and
re-derive the tag/band predicate from it. Both halves of that placement are
load-bearing. It cannot go *after* the guard call, because the deref block's
contract is "no call from here to the end of the clone" and the refresh can
itself allocate (`clean_arr_ptr` force-materialises a lazy array) — a refresh
there would reintroduce the very "base derived across an allocating call"
hazard step (4) exists to prevent. And it cannot skip the **write-back**,
because the query and deref blocks both deliberately re-read the binding to
survive a move by the guard call, and would pull the stub straight back out.
The write-back also lands the durable half of #6904's self-heal: after the
first visit the binding holds the live head, so later loop entries and the slow
clone address the current array directly. Closure-captured arrays are now
declined, since their capture cell is not updated by a plain slot store.

That the binding really was a stub rather than a live head is established
directly, not inferred: adding a module-scope `keep.push(…); keep.pop();` after
the `build()` — which forces a write-back of the *resolved* head into the
global — makes the same pre-fix compiler print the right answer and exit 0.
(It also follows by construction, since `js_array_refresh_local_head` returns
its input untouched when there is nothing to follow, so on an already-live
binding this fix would be a no-op and the crash would survive it.) A
**producer**-side gap is therefore also open, tracked as #7661:
`expr/array_push.rs` writes the reallocated head back to the pushing scope's
own slot, so the stub is being reintroduced somewhere between that slot and the
caller's binding. The consumer fix is the right layer regardless — a stale head
can arrive by several routes, which is exactly why every runtime entry point
resolves the chain.

**Why nothing caught it.** Every existing case — gap test and codegen census
alike — built its array in the same scope that read it, so the binding always
held the live head, and the largest was 64 elements pushed at module scope,
where each `push`'s write-back updates the global. The raw-pointer derivation
was never handed a stub. The new gap case covers both bound forms, so a future
narrowing of ingredient (2) cannot silently drop the coverage. `test_gap_repsel_element_shape_loop_clone.ts` gains
case 10 with both stale-head shapes above 16 elements (callee builds and
returns; callee grows the caller's array): **exit 138 on the pre-fix compiler,
byte-identical to node with the fix**. Three codegen census tests cover the
emitted form; the load-bearing one asserts the value stored back *is the
refresh's result* rather than merely that a store exists, and is
sabotage-checked by deleting the write-back arm.

Found while re-measuring #7480, whose own object-literal kernel
(`keep: {v,w}[]`, 200k × 50 sweeps) is unaffected by this path and re-measures
at 414 ms against node 12 ms / bun 12 ms on the pinned quiet host — the
issue's recorded 93 ms / 6.2× is stale, and the object-literal element type is
still the open half of the gap (`element_class_name` resolves only
`Array(Named(C))`).
