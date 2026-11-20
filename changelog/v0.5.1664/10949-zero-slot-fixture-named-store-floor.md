Fixed a zero-slot GC test fixture having no room for the named-store floor
(#10941).

`alloc_{nursery,old}_test_object(0)` allocated exactly an `ObjectHeader` and
left the receiver unstamped, on the reasoning recorded above it that "a
zero-slot fixture needs no descriptor at all — the derived bound is 0 either
way".

A named-property write does not respect that bound. The inline/overflow
boundary is `max(object_live_slot_count(obj), INLINE_SLOT_FLOOR)` and the floor
is 2, so the first two keys written to a zero-slot fixture store into inline
slots 0 and 1 of an object that has none — and those two words are the next
cell. Every caller before #10938 only ever set a `[[Prototype]]`, so nothing had
written a named property and the hazard was invisible; it presents as a wrong
read now and a SIGSEGV somewhere unrelated later.

Both fixtures now allocate `max(field_count, INLINE_SLOT_FLOOR)` slots while
**publishing** the bound as `field_count`, so the collector still traces exactly
`field_count` slots and the descriptor-count accounting the original comment
protects is unchanged. `gc::tests::zero_slot_fixture` asserts the allocation for
both fixtures and reddens by name if it regresses.
