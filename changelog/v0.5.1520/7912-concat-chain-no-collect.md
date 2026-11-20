### `iso_miss` −16% — a chain of heap strings concatenates without transient roots

`js_string_concat_chain` rooted every part into `RUNTIME_HANDLE_STACK` before
allocating the result and re-read every one of them afterwards, because
`string_storage_alloc` can collect and a copying minor would move the parts out
from under the copy loop. Darwin has no local-exec TLS, so each `thread_local!`
access is an `_tlv_get_addr` **call**; with the `RefCell` borrow and the `Vec`
push that is ~10 round trips per 4-part chain. On `gc-handoff/apps/iso_miss.ts`
— a tree-walking interpreter whose environment lookup appends
`seen = seen + "[" + names[i] + "]"` per frame, ~9 M times — xctrace put
`RuntimeHandleScope::root_string_ptr` at **8.48%** and
`RuntimeHandle::get_raw_const_ptr` at **4.91%** of the whole program: more than
the concatenation they were protecting.

The roots are unnecessary whenever the allocation cannot collect, and the
runtime can already tell. `arena_cell_alloc`'s first step is
`try_alloc_current`, a pure bump of the block that is already open; everything
past it (`gc_check_trigger()`, the cross-block scan, `reserve_arena_block`) is a
collection point or can reach one. **A successful `try_alloc_current` is
therefore a proof that nothing moved.**

New `arena::arena_alloc_gc_no_collect` and `string::string_storage_alloc_no_collect`
allocate or **refuse** — they never reach the collection point.
`js_string_concat_chain` grows a fast arm that admits only chains whose every
part is already a live heap string (those need no `js_jsvalue_to_string`, so
classification allocates nothing) and allocates through it, with zero handle
operations. On a refusal it falls through to the original rooted path: a
refusal is not an event, nothing has collected, so the operands are still
readable where they were. The admission scan runs before the sizing scan and
touches nothing but the `parts` array, so a mixed chain reaches the rooted path
having paid n register compares rather than n cold `StringHeader` loads it is
about to discard.

Retired instructions (`/usr/bin/time -l`, best-of-N, exit-checked; the dev host
was at load 30–200, where wall clock cannot resolve this): **`iso_miss` 0.836**,
`asyncpipe` 0.983, and the other 17 corpus programs 0.997–1.001. `interp` is
0.9998 — the same program without the trace-string instrument, which is the
control this change predicts.

★ Two things worth carrying forward. **The whole-corpus instruction sweep caught
a +5.5% `pipeline` regression that the targeted A/B would have shipped**: the
first cut reached the new primitive by refactoring `arena_alloc_gc` into a
`const MAY_COLLECT: bool` generic and routing `arena_cell_alloc`'s first
statement through a call — two functions every allocation in the program goes
through, both `#[inline]`, both "should" have been free. GC schedules were
identical across the arms (`PERRY_GC_DIAG=1`: 12 copying minors / 6 steps /
6 drains), so it was pure mutator work. Both are now byte-for-byte `main`'s and
the no-collect entry is written out separately. **And the first version of the
safety test could not fail**: "a small concat reached no GC trigger" is vacuous,
because a small allocation into a block with room does not reach the trigger
through the *collecting* allocator either — swapping the entry's body for
`arena_alloc` left it green. The tests now fill the block until the two entries
must diverge, and that sabotage turns two of them red.
