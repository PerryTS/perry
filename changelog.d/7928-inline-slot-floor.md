### perf(runtime): right-size small objects — `INLINE_SLOT_FLOOR` 4 → 2

Closes the front half of #7916 and all of #7714.

**The accounting.** A two-field object literal `{a: number, b: number}` occupied **72 bytes
to store 16 bytes of payload**: 8 `GcHeader` + 32 `ObjectHeader` (`object_type` 4,
`class_id` 4, `parent_class_id` 4, `field_count` 4, `keys_array` 8, `meta` 8) + 4 × 8 slot
bytes, of which only two slots are reachable. Alignment and capacity rounding contribute
**zero** — `ObjectHeader` is `#[repr(C)]` with no interior padding and
`gc_padded_total_size(64, 8)` finds `8 + 64` already 8-aligned. 22.2% of the allocation was
payload, 22.2% was the slot floor. `gc-handoff/bench/retain.ts` wrote 216 MB to store 48 MB
of doubles.

**Why the floor is a dial, not a safety constant.** Its doc comment called it
corruption-critical and 55 runtime sites plus 3 codegen sites independently compute
`max(field_count, FLOOR)` as the inline/overflow boundary — but the by-name append path
(`field_set_by_name/tail.rs`) only bumps `field_count` for a slot it placed *inline*, and
spills anything at or past `alloc_limit` to overflow storage. `alloc_limit` is therefore a
fixed point of the allocation and can never grow past the physical slot count, at any
FLOOR ≥ 0. (#6712 moved it 8 → 4 on the same reasoning.) 2 rather than 1 or 0 because all
three are indistinguishable in footprint for every shape in the perf corpus, so 2 keeps the
most inline headroom for a dynamically-grown `{}` at zero byte cost.

**Result.** `{}` / 1-field / 2-field literals 72 → **56 bytes**; 3-field 72 → **64**; ≥4
fields unchanged (their overhead is entirely the two headers). `retain` writes 168 MB
instead of 216 MB — write amplification **4.5x → 3.5x**. Peak RSS (bit-exact run to run):
`tree` −18.6%, `retain` −15.3%, `retain1` −12.8%, `deeplist` −11.3%, everything else ≤0.4%.

**The interaction worth knowing about.** `retain1` and `deeplist` retire 12–14% *more*
instructions, and none of it is mutator cost. Every minor GC in both arms fires at the same
byte mark and processes the same bytes — but 1.286 = 72/56 times as many *objects*
(`retain1` minor 1: 245 752 → 315 969 objects at 17 694 064 → 17 694 216 bytes). GC pause
39.60 → 50.10 ms, which exceeds the program's entire cycle delta: the mutator got faster and
the collector got slower, at an unchanged ~50 ns per promoted object. **The collector's
trigger is denominated in bytes; its cost is denominated in objects**, so every future
object-shrinking change is taxed back until the nursery/promotion budgets carry an
object-count term. Total promotion work is set by the surviving object count (unchanged), so
these microbenchmarks are seeing work pulled *forward* into their measurement window, not
created. The rest of the corpus moves the other way: `churn` −1.2%, `churn_alloc` −1.4%,
`push_cls` −1.4%, `tree` −0.8% instructions.

**Codegen pairing.** perry-codegen carried two separately-spelled `4`s held together by a
comment, used for opposite purposes: sizing the inline-`new` bump allocation (too small →
writes past the allocation) and emitting the property bounds checks (too large → reads past
it). Both now read `target_layout::INLINE_SLOT_FLOOR`, paired with the runtime by
`inline_slot_floor_matches_runtime` / `inline_slot_floor_matches_codegen`, the mechanism
`PIC_CACHE_WORDS` already uses.

**Validation.** 19/19 corpus programs byte-exact vs `node --experimental-strip-types`
26.5.1 with exit 0, and again under `PERRY_GC_PROTECT_FROMSPACE=1
PERRY_GC_VERIFY_EVACUATION=1` (a layout change is GC-visible); `iso_miss` canary
`checksum 437840 misses 0`; gap suite; `cargo test --release -p perry-codegen
-p perry-runtime`. New tests: `two_field_literal_footprint_is_exactly_accounted` reads the
size the *allocator recorded* in `GcHeader::size` rather than recomputing the formula, so it
fails if any allocation path stops honouring the floor, and
`by_name_growth_past_the_floor_reads_back` pins that the inline/overflow boundary stays
invisible to reads.

Full byte-level write-up and the projection for shrinking `ObjectHeader` itself:
`gc-handoff/REPR-NOTES.md`.
