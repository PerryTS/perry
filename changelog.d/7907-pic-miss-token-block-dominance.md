### Performance

**The generic property-get IC's miss block re-derived the whole receiver ladder.**
`interp.ts` retires **11.2% fewer instructions**, `iso_miss.ts` **7.6% fewer**, and
`evalNode`'s emitted code shrinks from 6516 to 5348 instructions.

#7883 routed all four of the guard chain's failure edges — small-handle receiver,
non-object receiver, MRU token mismatch, cached slot out of bounds — into a single
`pic.miss` block. That left `token`, `token_nonnull` and `epoch_eq` live on only
some of those edges, so the block **recomputed** them: four header loads and
compares, the `keys_array` and `parent_class_id` loads, the token select, a second
pair of `cache[2]` / `@PERRY_IC_EPOCH` loads, and a `select` substituting a safe
address for a small-handle receiver.

It was justified as cold. It is not cold. On a site whose receiver rotates over
more shapes than the MRU entry holds — the shape #7753's polymorphic ways exist
for, i.e. every discriminated-union dispatch — that block runs on nearly every
read. An `xctrace` profile of `gc-handoff/apps/interp.ts` put the **single hottest
instruction in the whole program** inside the recomputation: the `csel`
materialising `max(field_count, INLINE_SLOT_FLOOR)`, at 4.65% of `evalNode`, itself
56.6% of the program. (`sample` cannot profile a deeply recursive function and
attributes that time to the return addresses of `evalNode`'s own recursive calls.)

Two of the four edges are receiver-validation failures, and a receiver that fails
them can never resolve a way — `way_hit` ANDs `is_object` in, so the compares were
dead work for it. Routing just those two to a new `pic.miss.cold` (which records the
same two typed-feedback counters and goes straight to `js_object_get_field_ic_miss`)
makes `pic.miss` **dominated by `pic.token`**, and every re-derived value is
deletable: they are the values that block already computed, from the same memory
with no intervening store, and `is_object` is statically true.

Two smaller changes ride along, both value-preserving:

* The cached-slot bound is spelled `slot < INLINE_SLOT_FLOOR || slot < field_count`
  instead of `slot < max(field_count, INLINE_SLOT_FLOOR)`. Identical predicate
  (`x < max(a, b)` ⟺ `x < a ∨ x < b`), but the `max` had to be materialised and its
  `csel` sat on the dependency chain out of the `field_count` load; LLVM folds the
  disjunction into `cmp` + `ccmp`, and the `slot < 4` half does not depend on the
  load at all.
* The way `(token, slot)` match reduces as a balanced tree rather than a left fold,
  halving the depth of the `select` chain whose last node feeds the bounds compare
  that gates the branch out of `pic.ways`. At most one way can hold a given token
  (`pic_prime_get` evicts a duplicate before writing one, and a zero token is
  excluded by `token_nonnull`), so reassociating is value-preserving.

Codegen only — nothing under `perry-runtime` / `perry-stdlib` changes. Validated
with all 19 `gc-handoff` corpus programs byte-exact against
`node --experimental-strip-types` and exit 0, the `iso_miss` canary at
`checksum 437840 misses 0`, the whole corpus byte-exact under
`PERRY_GC_PROTECT_FROMSPACE=1 PERRY_GC_PROTECT_FROMSPACE_DEPTH=200
PERRY_GC_VERIFY_EVACUATION=1` with the instrument shown live (38 retired page-sets
on `interp`, 50 on `iso_miss`), and a differential run of the whole `test_gap_*`
suite compiled AND executed under both compilers with stdout and exit code compared.

Three new codegen contracts in `expr/property_get/tests.rs` assert the consequences
rather than the block names — one `@PERRY_IC_EPOCH` load per generic read, no
small-handle sentinel `ptrtoint`, no materialised `max`, and one lane select per way
— and all three go red against the pre-change lowering.
