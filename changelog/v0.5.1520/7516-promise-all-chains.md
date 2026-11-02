### Fixed

**`Promise.all` at scale rejected with a resolution value, or with
`TypeError: value is not a function` — eight stale from-space reads, none of them
in the promise machinery's *logic* (#7497).**

The app-pattern kernel `promise_all_chains` printed `Uncaught (in promise) 0`
under `PERRY_NO_AUTO_OPTIMIZE=1` and a rooting-shaped `TypeError` under the
default link, and was the last blocker on the public benchmark artifact. No
settle/reject/microtask *decision* changed. Every defect is the #7341 family —
*a value read out of a root and held in a register across a call that allocates
is not rooted* — and the fix is the same shape each time: `RuntimeHandleScope`
plus a re-read at the point of use, so the pre-collection address is never
nameable.

They were found one at a time. Each `PERRY_GC_PROTECT_FROMSPACE=1` fault named a
site; fixing it moved the fault and exposed the next.

1. **`js_get_global_this_builtin_value`** — the canonical `globalThis.<Builtin>`
   read behind `instance.constructor`, bare `Date`/`Array`/`Object` identifier
   resolution, and `is_default_promise_constructor`:

   ```rust
   let global_obj = js_nanbox_get_pointer(js_get_global_this());
   let key = js_string_from_bytes(name);            // ALLOCATES -- may collect
   js_object_get_field_by_name(global_obj, key);    // from-space deref
   ```

   The root is fine — `THREAD_GLOBAL_THIS` is registered and evacuation rewrites
   it. The ORDER is not: this lookup interns nothing, so every call mints a fresh
   string, and any of those allocations can be the copying minor that moves
   `globalThis`.

2. **`js_promise_resolve_spec`** — `Promise.all` calls it once per element, and
   it asks `is_default_promise_constructor` (i.e. 1) before touching its own
   argument. For `Promise.all([...promises])` that argument IS a promise, so
   `js_promise_resolved` dereferenced a from-space `GC_TYPE_PROMISE`.
   `js_promise_reject_spec`, `js_promise_try_spec`,
   `js_promise_with_resolvers_spec` and `promise_prototype_then_thunk` share the
   shape.

3. **`js_array_clone`** — the widest. It read the source array pointer, called
   `js_array_alloc(len)` for the destination (which can collect and MOVE the
   source), then `copy_nonoverlapping`'d from the pre-collection address. That is
   every `[...arr]`, every `Array.from(arr)` and every combinator's iterable
   snapshot.

4. **The spec combinators' own locals.** `perform` ran its per-element loop —
   `Call(promiseResolve, C, «next»)` and `Invoke(nextPromise, "then", …)`, both
   of which run USER JS — while holding the `elements` snapshot, the shared
   `values` and remaining-count arrays, the capability's `resolve`/`reject` and
   the constructor in bare Rust locals.

5. **Two publishers, worse than a stale read.** `build_element_closure` and
   `make_resolving_functions` take their GC arguments in registers, allocate, and
   then *store the pre-collection addresses into capture slots* — putting
   from-space into an object the collector goes on maintaining.
   `new_promise_capability`, the two `Promise.allSettled` element functions,
   `build_settled_{fulfilled,rejected}` and `combinator_iterable_to_array`'s
   array fast path had shape (4) or (5).

6. **The microtask runner's dispatch arms.** Every arm read its callback pointer
   (and the value it passes) out of the popped `Task` into a bare local, ran
   `async_hooks::before` / `v8::promise_hook_before` — both allocate — and only
   then loaded `func_ptr` out of that pointer. #1663 had already rooted `promise`
   and `next` here; the callback was missed.

7. **…and rooting them inside the arm was still too late.** A `Task` stops being
   a scanned root the instant it is popped, and `enter_microtask_context` runs
   before any of the arm's own bookkeeping. The first attempt seeded the handle
   with an address the collection had already invalidated. Every arm now roots as
   its first statement and re-reads after the context switch. Disassembling the
   faulting instruction — `bl get_nanbox_u64; ldr x8,[x21]` — is what showed the
   re-read was present and *still* wrong, which is what made the ordering the
   suspect rather than the rooting.

8. **The producer side.** `js_async_step_chain` carried the step closure through
   `adapt_foreign_promise_value`, `js_promise_new`, `build_async_step_thunks`,
   `js_promise_resolved` and `capture_context` and then STORED it into a
   `Task::AsyncStep`. The queue is a scanned root and the runner now re-reads
   what it pops — neither helps when the pointer was already dead at the push.
   `js_async_step_done` had the mirror image: it settled `trap_next` (which
   allocates) and returned the *pre-call* copy as the async function's own result
   promise.

Review (CodeRabbit) found six more of the same shape, all fixed here: the
SEARCHED closure value in `class_meta.rs`'s lookup loop (which *misses* rather
than crashing, so the caller silently falls through to the generic construct
tail); `perform_promise_then_with_cap`, which filled one wrapper's captures and
then allocated the other; `js_array_values`, carrying (3)'s shape;
`combinator_iterable_to_array`'s `GC_TYPE_OBJECT` arm, whose receiver crossed a
user `[Symbol.iterator]` getter; `js_async_step_chain`'s three early-return
suspend paths together with `then_backpatch_result` (which allocates the result
promise and then STORES it into both thunks); and one encoding mismatch between
`boxed_closure` and `rooted_closure` in the `AsyncStep` arm.

All handles are NaN-boxed rather than `root_raw_*_ptr`, so
`scripts/raw_handle_debt.py` is unchanged at 999. The per-element handles in
`perform` live in a scope INSIDE the loop, so a 50 000-element combinator does not
push 50 000 entries onto the handle stack.

**Three more callers of `js_get_global_this()` had (1)'s shape** and are fixed
the same way. These come from auditing the callers, not from a reproducer, and
are called out as such: `class_meta.rs`'s builtin-constructor name walk (worse
than the proven site — a fresh key allocation inside a ~50-iteration loop against
one address read before the loop), `js_globalthis_seed_async_local_storage`
(`globalThis` is the RECEIVER of a store that follows two allocations), and the
`Temporal.<Type>.prototype` walk. The four sites of this shape in `error.rs` /
`with_env.rs` were already fixed by #6943; these are the ones that sweep missed.

**Why it read as "a separate promise-rejection defect".** A stale read returns
whatever from-space happens to hold, so `globalThis.Promise` came back as a
non-callable — or, when the garbage was zero, as the resolution value `0`
arriving on the rejection path. The two link modes printed different messages for
the same defect. It was untouched by #7495 only because #7495 fixed a different
function.

**Localisation, for the next person** (each knob against the unfixed binary):
`PERRY_GEN_GC=0` and `PERRY_WRITE_BARRIERS=0` both make it pass while
`PERRY_GC_MOVING_SAFEPOINT=0` does not — the first two make the copying minor
ineligible, the third only disables the *safepoint* collection, and the one that
matters is the alloc-point direct minor (`trigger=ArenaBytes
declared_safepoint=false`). `PERRY_GC_FROMSPACE_SCAN=1` reported `clean` every
time: no HEAP slot was ever stale, which is the signature of a holder in a native
frame rather than in a table.

**What is still open, stated rather than papered over.** A protected run of the
*auto-optimize* binary is not silent: it prints the correct checksum and then
faults inside `js_async_step_done` on another 72-byte `GC_TYPE_PROMISE`. Three
separate attempts at it are in this PR and all three are correct on their own
terms — returning the re-read address instead of the pre-call copy, rooting the
receiver across `js_assimilate_thenable`, and seeding `INLINE_TRAP` from the
handles rather than from the arm's locals — and **none of them cleared it**, so
the holder is somewhere I have not found. It is after the program's observable
output; the kernel matches the oracle byte for byte with and without the
instrument, and the `PERRY_NO_AUTO_OPTIMIZE=1` binary and the new gap test are
both silent under it. Separately, `test_gap_gc_iterator_drain_rooting` and
`test_gap_iterator_helpers_2874` fail on `origin/main` too — verified by building
`origin/main`'s runtime from a clean `git archive` export and running both
against it — and belong to #7498's `array_from_spread_value` prototype walk.

### Added

**`test-files/test_gap_gc_global_builtin_lookup_rooting.ts`**, registered in
`test-parity/gc_repsel_corpus.txt` so `gc-moving-witnesses` runs it. One wide
`Promise.all` (50 000 elements) rather than the kernel's 1000 × 50: the single
wide call packs enough lookups between two collections to fail on the shipped
default in a fraction of a second, where the kernel needs ~20× the work for the
same window. Deterministic — 6/6 runs failing before, 5/5 passing after — and
byte-diffed against node 26.5.1.

Verified with the instrument asserted live rather than merely quiet: a protected
run prints `[gc-fromspace-protect] retired_set=#0` and
`[gc-copy-minor] ran copied_objects=175416`, so objects really did move, and no
fault follows.

### Changed

`scripts/auto_opt_app_patterns.sh` no longer skips `promise_all_chains`; the skip
list is empty and the gate is 12/12. Its rot check means the line had to come out
with the fix. Every array expansion is now guarded on `${#…[@]}`: macOS ships
bash 3.2, where `set -u` turns `"${EMPTY[@]}"` into an "unbound variable" abort,
so an empty skip list would have stopped the gate before its first kernel —
CLAUDE.md hazard 4 wearing a different hat. `--self-test` still passes.
