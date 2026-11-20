Made the two hot shadow-stack runtime operations cheap. A ceiling measurement
had put the shadow stack at +71.5 % instructions on application-shaped code
(`w6_records`, Pi 5) versus not emitting it at all — roughly 65–70 instructions
per shadow op — and located two implementation costs rather than anything
inherent to shadow stacks.

**`js_shadow_frame_push` did three `Vec::resize` calls per activation.** The
per-slot state lived in three parallel thread-local `Vec`s (`stack: Vec<u64>`
values, `slot_ptrs: Vec<usize>` bindings, `active: Vec<bool>` liveness bits),
so one frame push meant five capacity checks, three length updates, and — read
out of the linked archive's disassembly — up to three `memset` **calls** for a
frame with two to four pointer-typed locals. The three words are now one
16-byte `ShadowEntry { value, meta }`, the frame header packs `prev_frame_top`
and `slot_count` into a single entry, and the slot clear is a constant-size
store. A push is now one capacity check plus `movi`/`stp q0, q0`/`stp q0, q0`,
with no call on the common path; a slot store is one bounds check against one
length instead of three, and writes the whole entry with one `stp`.

Keeping that clear call-free took three attempts, all verified by
disassembling the shipped archive: LLVM re-forms a `match` on the slot count,
a bounded loop, and a runtime-length `write_bytes` alike into a compare chain
that calls `memset`, and it tail-merged even the constant-size store with the
neighbouring large-frame `memset` into one `csel`-the-length-then-call until
the large path moved behind `#[cold] #[inline(never)]`.

**`js_shadow_slot_set` / `js_shadow_slot_bind` called the root write barrier on
every store.** That barrier is the incremental-mark root *shading* barrier, not
the generational remembered-set barrier — old→young edges are logged by the
heap-slot barriers, which are untouched. Its entire body reads a thread-local
and returns when no incremental cycle is armed, so the cost was a call plus a
second TLS lookup on every pointer-typed local update. It is now gated inline on
`PERRY_INCREMENTAL_MARK_BARRIER_ACTIVE_COUNT`, the same guard codegen already
emits around `js_write_barrier_root_nanbox` for persistent shadow slots. The
gate is observationally identical rather than a narrowing:
`incremental_mark_barrier_enable` installs the thread-local *and then*
increments the count before returning to the mutator, so a zero count proves
this thread's pointer is null and proves the call would have been a no-op. A
non-zero count is conservative in the harmless direction. The barrier itself is
unchanged and still fires whenever a cycle is in flight.

The liveness bit now shares a word with the bound compiled-local address (bit 0,
always free on an 8-byte-aligned local slot). An address that would collide with
the tag is recorded as active-but-unbound rather than truncated: the mirrored
value is still marked and still rewritten, and the collector is never handed a
mis-derived address to write a forwarded pointer into.

New `gc::tests::shadow_stack_ops` covers the four properties an optimisation
here can silently break — liveness, rewritability through a real copying minor,
the value the mutator actually stored, and a fresh frame never inheriting the
recycled buffer tail — and every one was verified to fail under a targeted
sabotage of the thing it covers.
