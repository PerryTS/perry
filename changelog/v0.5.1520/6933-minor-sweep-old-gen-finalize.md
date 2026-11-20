fix(gc): a minor sweep no longer finalizes unmarked old-generation objects (#6892).

A minor trace never marks the old generation — old-gen parents are black
leaves, visited only through dirty remembered-set pages — so "unmarked" in a
minor means *unvisited*, not *dead*. `ArenaSweepObjectsState::process_object`
sent every unmarked old-gen object down `reclaim_dead_object` anyway. A minor
never frees old-gen memory, so this stayed latent; the damage was
`finalize_dead_arena_payload`, whose `layout_clear_for_ptr` dropped the live
object's `LAYOUT_SLOT_MASKS` entry (plus its payload side tables and external
payload buffers).

That left a live old-gen object in `GC_LAYOUT_SIDE_MASK` state with no mask.
The next `layout_note_slot` rebuilt the mask from that single slot, so the
following trace visited one element and skipped the object's other pointer
slots — sweeping children that were still referenced. Reads then hit a
recycled address and returned `undefined`, which is why the reported symptom
was a varying `Cannot read properties of undefined (reading '<field>')`.

`unmarked_is_provably_dead()` now gates reclaim on "the trace covered this
object's generation": old-gen blocks are exempt during a minor sweep, except
blocks selected for old-page defrag in the same cycle, whose live contents
were evacuated by that cycle. Full traces are unchanged. The sweep's
`retain_all_forwarded_stubs` flag is renamed `minor_sweep`, which is what
every call site already passed and what both retention rules now key off.

Found via the Milo compiler built by Perry: `emit-ir` over a file importing
`std/fetch` threw, and its LLVM IR is now byte-identical to the same compiler
under bun. 51/53 of milo's examples now match byte-for-byte.

Side effect worth knowing: minors used to report every unmarked old object as
dead bytes to `old_page_account_swept_object`, i.e. phantom fragmentation
derived from mark bits that say nothing about the old generation. They now
report those objects live, so the old-page defrag selector's dead-byte signal
comes from full traces, where it is real.
