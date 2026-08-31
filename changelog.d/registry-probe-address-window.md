**The two hottest side-table probes answer "no" from an inlined address
compare instead of an out-of-line registry lookup.**

`is_registered_buffer` and `lookup_typed_array_kind` are asked "is this pointer
special?" from ~239 and ~200 generic call sites — every property get/set, every
prototype walk, every element read, every `[[HasProperty]]`. #9176 gave both a
monotone "has anything ever been registered?" latch, which makes the answer free
for a program that never allocates a `Buffer` or a typed array. `claude-code`
allocates both, so the latch is armed and stops discriminating.

How badly it stops discriminating was the finding. Counted with uprobes on a
symbolized `claude --help` (12 reps for the profile, exact counts from one run):

| probe | registrations | calls | answered "yes" |
|---|---|---|---|
| `is_registered_buffer_slow` | 10 | 4,651,086 | **4** |
| `lookup_registered_typed_array_kind` | 42 | 3,567,647 | **0** |

465,000 probes per registered buffer, and 85,000 per registered typed array,
every one of them going out of line to a thread-local resolution, a `RefCell`
borrow and a hash — for the typed-array probe, also a direct-mapped negative
cache whose cold miss *writes back* and dirties a shared cache line.

`RegistryAddrWindow` is the same monotone idea applied to the address rather
than to the fact of registration: a process-global `[lo, hi]` that every
registration widens *before* it publishes, checked inline at the call site. An
address outside it cannot be in any table the window covers, so rejecting is
sound; accepting falls through to the exact lookup that was already there. It is
strictly stronger than a latch — an unregistered process has the empty window
`[usize::MAX, 0]`, which contains nothing.

It is deliberately *not* a `GcHeader` tag test. A registered typed array is not
required to have a readable `ptr - GC_HEADER_SIZE` (see the `mprotect`ed
guard-page fixture in `promise::combinators`), and `native_arena`'s
`native_memory_copy_rejects_buffer_registry_forged_to_old_non_buffer` pins that
a registry entry may legitimately disagree with the header type. The window
never dereferences the candidate, so neither case can be misclassified.

`buffer::header` already had this filter as the thread-local `BUFFER_ADDR_RANGE`
— but *behind* the call, where it rejected 87.8% of probes and still paid the
call, the prologue, two thread-local resolutions and a tail call into
`is_shared_sab` for every one of them. The window is the same test hoisted in
front of the call and widened to cover the external and `SharedArrayBuffer`
registries too, so those routes keep their existing behaviour.

Measured on `claude-code` (`cli_2.1.112.js`, `--help`, both arms built from
`b3f14e9cde` in one session, `PERRY_DEBUG_SYMBOLS=1`, 9 interleaved reps):

<!-- NUMBERS -->

Output stays byte-identical to `node cli_2.1.112.js --help` (9,175 bytes, rc=0).

Left for a follow-up, both measured on the same run and reported with it:
`closure::dynamic_props::is_closure_ptr` (1.34%) consults no registry at all —
its cost is the four-way arena page-range cache inlined from
`classify_heap_generation`, which runs before the `CLOSURE_MAGIC` test that
would reject 89.7% of its callers; and `object::prototype_chain::meta_capable_object`
calls `is_registered_buffer` immediately before an `obj_type == GC_TYPE_OBJECT`
check that already excludes every buffer with a header.
