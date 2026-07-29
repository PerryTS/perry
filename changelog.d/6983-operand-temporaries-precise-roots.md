### Fixed

- **GC: operand temporaries in three more lowering paths are precise roots (#6969, #6970, #6971).**
  #6972 rooted variadic argument accumulators, concat operand pairs and literal
  element lists; #6975 closed the coercion hole in the gate. Three sibling paths
  still kept an evaluated operand in a bare LLVM SSA register across a
  collection point, which under precise-roots-only
  (`PERRY_CONSERVATIVE_STACK_SCAN=off`) is a live use-after-free:

  - **#6970 — collection-method operands.** `m.set(fresh(0), churn(N))`
    **aborted** (exit 134, `grown Map must retain its side-allocation owner
    record`): the key was finished and live only in a register across the
    value's lowering, so `js_map_set` ran against a header the sweep had freed
    and `churn` had reused. Fixed in the `Expr::MapSet` / `MapGet` / `MapHas`
    lowering and in the `PropertyGet` dispatch that handles non-`Ident`
    receivers (`this.field.set(…)`), including `Map`/`Set`/`URLSearchParams`
    `forEach`, whose callback closure is itself an allocation the receiver has
    to survive.
  - **#6969 — constructor arguments.** `new Pair(fresh(0), churn(N))` held
    argument 0 across argument 1's lowering *and* across the instance
    allocation, which always collects.
  - **#6971 — string-method receiver and the `concat` accumulator.**
    `fresh(0).concat("|" + churn(N))` dropped its receiver. `concat` is the
    dangerous form: its accumulator is a bare `StringHeader*`, the word form
    only `gc::root_words`' bare case covers, and every `js_string_concat`
    returns a *new* address — so the slot is written back with
    `js_gc_temp_root_set`, not merely re-read.

  Two mechanism additions, both built from #6972's primitives:
  `RootedOperands` (root already-lowered operands when the *caller* knows what
  follows collects — a per-branch operand representation for `MapSet`, an
  allocation for `new`), and `temp_root_scope_begin`/`_end`, an
  expression-scope barrier. The barrier is what makes `lower_new` tractable:
  truncation is a stack *cut*, so one marker slot releases the whole group on
  whichever of that function's ~20 return paths ran, instead of a
  `temp_root_release` at each that future edits must keep balanced.

  A new suppression, `operand_needs_root`, keeps this free where it was already
  safe: literals, module globals, provable non-pointers, and locals that
  **have a reserved shadow slot**. The shadow-slot check is load-bearing — a
  blanket `LocalGet` suppression regressed #6970 straight back to an abort,
  because a local can be pointer-valued with no shadow slot and therefore no
  precise root at all (that is #6968).

  A temp root buys two things — liveness, and a location the collector rewrites
  — and the suppressions only give up the first. A suppressed operand reads a
  registered root, so it can never be *swept*; but evacuation **rewrites that
  storage**, leaving the pre-collection register pointing at where the object
  used to be. `operand_is_reloadable` therefore re-emits the load at the
  re-read point instead of reusing the register: correct under relocation, and
  a plain `load` rather than a runtime call. This is the same staleness #6981
  reports one layer in, for a raw typed-array pointer under the specialized
  ABI.

  Cost: on a probe of already-safe shapes (`"user_" + i`, `[1,2,3]`,
  `{a:i,b:total}`, all-local argument lists, `m.set(k, 1)`, `m.get(k)`,
  `label.slice(1,3)`, `new Pair(label, i)`) the emitted LLVM IR is
  **byte-identical** to `main`, md5 included. The three protected shapes add 11
  runtime calls and 14 IR lines in total. Where a real allocation does intervene
  over a registered-root operand, the cost is one extra `load` per operand and
  no runtime call.

  Verification: each issue's reproducer is byte-exact over 4 runs under
  `PERRY_CONSERVATIVE_STACK_SCAN=off PERRY_GC_HEAP_LIMIT=8` with the arm
  measurably live (`PERRY_GC_TRACE=1`: 22 completed cycles), against exit-134 /
  silent-DIFF before. Rooting a constructor's argument list *after* the lowering
  loop rather than interleaved turned #6969's silent DIFF into a SIGSEGV — it
  publishes an already-dangling pointer to the scanner — so the interleaving is
  pinned by a test.

### Notes

- `gc::tests::temp_roots::rewriting_a_slot_roots_the_new_value_and_releases_the_replaced_one`
  pins `ConservativeStackScanMode::Disabled`, as every test in that module must:
  with the unit-test default (`Full`) the native-stack scan finds the raw
  pointers in the test's own Rust locals and the test passes without proving
  anything about precise roots.
- Six codegen IR tests pin the emission contract and, just as importantly, the
  *absence* of rooting on the shapes that were never broken.
