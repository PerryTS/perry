### `fix(gc)`: refuse a forwarding walk out of, or into, something that is not an object — and make the rekeyed-table prune structural (#8174)

`GC_FLAG_FORWARDED` means "the first payload word is where this object moved
to". Both forwarding walkers — `CopyingNurseryCollector::rewrite_raw_addr` and
`gc::verify::try_rewrite_raw_addr` — trusted that byte for **any** address in a
known heap region, and trusted whatever word they found behind it.

For a slot the collector already proved is a live reference, both are safe. For
a **metadata key** they are not. `RuntimeRootVisitor::visit_metadata_usize_slot`
and its siblings rewrite a recorded raw heap address if a moving collection
forwarded it and deliberately do **not** mark it — the key is a side table's
key, not a reference the program can reach, so rooting it would leak. The price
of that choice is that the key's object can die and the arena can recycle the
address under it.

#8040 is what that looks like, instrumented: recycled payload bytes at a dead
`FUNCTION_CLASS_IDS` key presenting `gc_flags = 0x86` (`GC_FLAG_FORWARDED` set by
coincidence), `obj_type = 104` — a type id no `GcTypeInfo` entry exists for — and
a "forwarding pointer" that was really a NaN-boxed value (`0x7FFF…`). The walk
followed it, could not classify the next hop, **stopped and returned it**, and
`visit_metadata_nanbox_key` masked it to 48 bits into a live, unrelated
survivor. A synthetic class's id was bound to an interned string, and the
program failed several collections later, in a different function, with
`TypeError: value is not a function`.

#8168 removed that one dead key. This closes the following.

**Two discriminators, one at each end of the hop** (`gc/forwarding.rs`, new):

* `forwarding_walk_header` refuses to read a forwarding pointer out of an
  address that does not read back as a real arena object header
  (`plausible_gc_header`: registered `obj_type`, sane size, `GC_FLAG_ARENA`).
  #8040's recycled bytes fail on `obj_type = 104`. Every real forwarding source
  passes: `set_forwarding_address` overwrites one payload word and ORs one flag
  bit, and all four production installers (`copying::move_young`, promotion,
  `gc::oldgen` defrag, and `array::push_pop`'s growth stub, which requires
  `GC_FLAG_ARENA` at install time) operate on arena objects. This is **not** the
  `self.ptrs.classify()` gate that `rewrite_raw_addr`'s own doc records as
  having un-rekeyed legitimate `shapes.entries` keys — that one additionally
  narrows on SPACE and resolves the survivor thread-locals; the header test
  carries none of that.
* `accept_forwarding_target` refuses a target that is not the start of a heap
  object, so a bogus word can no longer become the answer by virtue of the walk
  merely stopping at it. On a registered arena range the target must read back
  as a real object header; off-arena (an array-growth stub whose new head
  outgrew the arena) it must at least pass `is_plausible_heap_addr`, which
  rejects the handle band and everything at or above `HEAP_MAX` — which is what
  the `0x7FFF…` word fails.

Both are applied to the verifier too. That is not symmetry for its own sake:
`try_rewrite_raw_addr` is what `RuntimeRootVisitMode::Verify` runs, and it panics
whenever it can rewrite a slot the rewrite pass left alone, so tightening only
one walker would have converted a silent corruption into a
`PERRY_GC_VERIFY_EVACUATION` abort blaming an innocent scanner.

Refusals are counted and, under `PERRY_GC_DIAG=1`, reported as
`[gc-forwarding] <phase> refused_sources=… refused_targets=…` — printed **only**
when non-zero, so a healthy run adds nothing to any log a gate parses and a line
is a positive report that a stale forwarding header reached a rewrite walk.

**The structural half.** `gc::dead_owner` is the real fix for this class: drop
the entry before its dead key can be walked. Its fan-out covered a dozen tables
and #8168 made it thirteen, but nothing checked that the list was complete —
the invariant was maintained by hand, and a rekeyed table added without a prune
reopens #8040 in a form that takes days to trace back.

* `DEAD_KEY_PRUNES` (`gc/dead_owner.rs`) is now the registry `fan_out` iterates,
  19 entries, each naming the tables it prunes and which of the pass's three
  deadness predicates it takes.
* `scripts/gc_rekeyed_key_tables.py` (wired into `lint`, a required context)
  enumerates every `visit_metadata_*` call site in `perry-runtime` /
  `perry-stdlib` — 37 of them — and requires a written verdict for each in
  `scripts/gc_rekeyed_key_tables.json`. A `dead_owner:<fn>` verdict must name a
  prune that is actually in the registry; a `self_pruned:<fn>` verdict must name
  a function that exists; a new unclassified site fails; **an exemption that
  matches nothing fails too**, so a fix must delete its own entry. Floors on the
  site count and the registry parse make a broken regex exit 2 rather than
  report a clean, empty, green run. `--self-test` plants twelve shapes — a new
  site, a stale entry, a prune deleted from the registry, a `self_pruned` naming
  a ghost, a verdict with no reasoning, an over-cap gap, an issue-less gap, both
  floor rots, and a doc comment that must NOT count as a site — and requires the
  checker to adjudicate each correctly.

**What the audit found, and what happened to it.** The gate's first run turned
up six more rekeyed tables with no death story — live #8040 exposure, in six
places, none of them previously named. All six are fixed here, so the manifest
lands with **zero** declared gaps and `MAX_OPEN_GAPS = 0`:

| table | fix | issue |
|---|---|---|
| `CONSOLE_INSTANCES` | `prune_dead_console_instance_owners` | #8190 |
| `BOXED_PRIMITIVE_PAYLOADS` | `prune_dead_boxed_primitive_payload_owners` | #8191 |
| `TRANSITION_CACHE_GLOBAL` (`prev_keys`, `key_ptr`) | `prune_dead_transition_cache_entries` | #8192 |
| `ASYNC_STEP_GUARD.last_closure` | **field deleted** | #8193 |
| `REFLECT_METADATA.target_bits` | `prune_dead_reflect_metadata_targets` | #8194 |
| `SYMBOL_ACCESSOR_PROPERTIES` (owner half) | folded into `prune_dead_symbol_property_owners` | #8195 |

#8193 is the one that is not a prune. `AsyncStepGuard::last_closure` held the
address of the closure that took the last erroring async step, for a
same-closure check that was **deleted** when #712/#921/#922 showed a runaway
loop alternates between two closures. Nothing has read it since. It was not
inert, though: it was a raw heap address the promise scanner rekeyed without
marking, and nothing pruned it. Writing a prune to maintain state nobody reads
would be its own dead code, so the field goes, and with it the
`PROMISE_SCAN_ASYNC_STEP_GUARD` budgeted phase, whose only slot it was.

#8195 is not a new prune either: the accessor table shares its owner key with
`SYMBOL_PROPERTIES` and `SYMBOL_PROPERTY_ATTRS`, both pruned since the
2026-07-09 audit, and was simply left out. It now takes the same pass's
memoized owner verdict, so all three tables agree about every owner. That also
closes a leak — the accessor closures in a dead owner's descriptors were
immortal.

Each of the four new prunes has a pair of cases in
`gc/tests/dead_owner_side_tables.rs`: the prune **fires** (dead owner, one
collection, the table observably shrinks) and its inverse (a rooted owner's
entry survives — a prune that drops live entries is worse than the stale key it
removes). The transition-cache case allocates its rooted `next_keys` in OLD-GEN
on purpose: a reachable neighbour in the dead array's own nursery block would
force-mark it (#7975) and the prune would correctly decline, which would have
read as a failure of the prune.

**Tests** (`gc/tests/forwarding_target_validation.rs`, 5 cases). The two
sabotage cases plant #8040's shape verbatim and **assert the premise first** —
that the pre-#8174 gate would have accepted the planted bytes — so a green run
says the discriminator works rather than that nothing was tried. The premise
case asserts the opposite direction: a genuine evacuation still rewrites and
neither refusal counter moves, which is the property the rejected
`classify()`-based tightening broke. The registry case asserts `DEAD_KEY_PRUNES`
has not shrunk, that its labels are unique, and that #8168's `FUNCTION_CLASS_IDS`
entry is still present with its `GC_TYPE_CLOSURE` narrowing.

`scripts/gc_pin_sites.py` gains one allowlist entry: the planted `gc_flags =
0x86` carries bit 2, but nothing is being pinned — the address is payload
interior of a live allocation with no object at it, and rewriting the byte as
named flags would misreport what #8040 actually observed.
