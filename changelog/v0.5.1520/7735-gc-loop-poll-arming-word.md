### perf(gc): the loop back-edge poll's no-work path is now one global load (#7735)

`churn_alloc` 0.420 s -> 0.376, `push_cls` 0.409 -> 0.356, `push_num` 0.178 -> 0.144,
`churn` 0.458 -> 0.419, with `tree` 1.647 -> 1.634, `tree_wide` 2.111 -> 2.121,
`cycles` 0.196 -> 0.192, `deeplist` 0.320 -> 0.315 and `churn_read` 0.023 unmoved.
Best-of-5 with all arms interleaved in one session, quiet M1 bench host, outputs
verified byte-identical to `node --experimental-strip-types`.

#### What regressed, and it was not the pointer-field work

Three all-numeric benchmarks lost 15-30 % in the #7686..#7721 window, and the
obvious suspect was #7686/#7698's typed pointer-field stores generalising a path
that used to have an all-numeric special case. Measured, that hypothesis is
**refuted**: a symbolicated profile of `churn_alloc_big` at 0.5.1384 and at main
has the same shape symbol for symbol — `gc::layout::init_typed_shape_layout` 23.0 %
-> 21.8 %, `layout_forget_object` 7.1 % -> 8.1 %, the user constructor 24.0 % ->
23.3 %. Nothing in the layout or store path grew.

What appeared instead were two symbols that are not in the 0.5.1384 profile at
all: `js_gc_loop_safepoint` at 8.2 % and `_tlv_get_addr` back at 8-9 %, having
been driven to 0 % by #7469. Both arrive with #7721, which turned the moving-loop
back-edge poll on by default.

#### Mechanism

#7721 was right about the collector. The poll is the only precise
nursery-collection point a compute-only program ever reaches, and without it every
nursery collection happens at the register-imprecise allocation point where #7682
made it correctly non-moving — a collector with no nursery evacuation at all, worth
`tree_wide` 7.26 s instead of 2.11. What was wrong was the poll's **price**.

A poll is emitted at every allocating loop back-edge: 20 million of them in
`bench/churn_alloc.ts`, 200 million in `churn_alloc_big.ts`. So its no-work path
is a per-iteration cost of the language, paid whether or not a collection is ever
due — and that path was an out-of-line `extern "C"` call into

1. `gc_moving_loop_polls_enabled()` — a `OnceLock` acquire load,
2. `note_loop_poll_reached()` — an **unconditional** `AtomicU64::fetch_add` on a
   process-shared line,
3. `GC_SAFEPOINT_PENDING.with(Cell::get)` — a thread-local read, which on Darwin
   is a CALL to `_tlv_get_addr`, Mach-O having no local-exec TLS model,
4. `gc_zeal_enabled()` — a second `OnceLock` acquire load,

plus the caller-side spill/reload the opaque call forces. ~3 ns per back-edge,
which is the regression to the millisecond: 20 M x 3 ns = 60 ms against a
churn_alloc gap of 51.5 ms.

#### The fix

`gc/poll_arm.rs` adds `PERRY_GC_POLL_ARMED`, a plain process-global `AtomicU32`
counting the reasons `js_gc_loop_safepoint` must do more than return. **Zero is a
proof the poll is a no-op**, so both ends can answer on one ordinary load: codegen
emits the load inline and branches around the call entirely, and the runtime entry
point re-checks it so a module from any other emission path still gets the cheap
answer. On aarch64 the emitted guard is two instructions with the address hoisted
into the loop preheader:

```
ldr  w8, [x26]        ; x26 = &PERRY_GC_POLL_ARMED, loop-invariant
cbnz w8, .gcpoll
```

The word is a deliberate conservative SUPERSET. It is process-global — a
thread-local would reintroduce the `_tlv_get_addr` this removes — so it counts
threads with a deferral outstanding, and a poll on thread B can be woken by a
deferral on thread A and find nothing to do. The unsound direction is the word
reading zero while a deferral is outstanding, which would strand that collection
until an event-loop boundary a compute-only program never reaches. That is why
`GC_SAFEPOINT_PENDING` now has exactly one writer, `policy::set_safepoint_pending`,
which moves both representations together; the `Cell` carries a "write it only
through the helper" note, and `a_deferral_arms_the_poll_word_and_draining_disarms_it`
pins the pair in both directions.

The load is **volatile**. The runtime writes this word from calls LLVM cannot see
through, and a guard whose load got hoisted out of its loop would read a stale zero
and silently stop draining — #7721's failure mode returning as a codegen bug
instead of a default. It is one `ldr` either way, so nothing is bought by leaving
it to alias analysis.

Zeal keeps the word armed for the life of the process. `PERRY_GC_ZEAL`'s contract
is a collection at every safepoint, not only at ones already deferred, and that is
expressible only with the word armed and nothing pending; released otherwise, by a
one-shot seed the first poll resolves (asking `gc_zeal_enabled()` on the fast path
costs exactly what the word exists to avoid). `ZealGuard` mirrors it so a unit test
under zeal cannot silently poll into a no-op, and `loop_polls_reached()` now says in
its own doc that it is exhaustive exactly under zeal — which is the one place
`zeal_verdict` reads it.

#### What is left, and why

The remaining gap to 0.5.1384 is 9.4 ms on `churn_alloc`, 6.2 on `push_cls`,
12.6 on `push_num` — 0.5 to 0.6 ns per back-edge, i.e. exactly the two guard
instructions, confirmed by disassembly rather than inferred. It is not overhead
that can be removed by tightening anything: it is the price of the poll existing,
and the poll is what makes the nursery evacuate. The 0.5.1384 numbers were produced
by a moving minor at the allocation point, which #7682 removed as unsound; compiling
and running today's main with `PERRY_GC_MOVING_LOOP_POLLS=0` — the 0.5.1384
configuration — gives `churn_alloc` 0.91 s, not 0.36. Driving the guard to one
instruction would mean a signal-backed polling page (`ldr wzr, [x26]` + an
mprotect'd page), which is not proportionate to 5 ms and is filed rather than done.

#### Tests

- `poll_arm::tests` — the counter is a counter and not a flag (a flag strands the
  second thread's deferral), and `disarm` saturates rather than wrapping, because a
  wrap to `u32::MAX` reads as armed forever and reinstates this whole regression
  silently and permanently.
- `an_unarmed_poll_touches_nothing` — the assertion that makes the change real
  rather than decorative: leaving `note_loop_poll_reached` above the gate would keep
  the single most expensive instruction of the old path on every back-edge while
  looking identical in every other test.
- `zeal_holds_the_poll_word_armed_with_nothing_pending`.
- `loop_safepoint_purity.rs::a_surviving_poll_is_guarded_by_the_arming_word` — the
  declaration, the volatile load, one guard per poll, and the call sitting behind
  the branch rather than after the load in the same block. Checked in IR because
  nothing else can fail when the guard goes away: the program stays correct, every
  other test stays green, and it is 15-30 % slower.

The call survives in the IR, which is what `gc_call_effects` classifies, what
`scripts/gc_root_dominance_check.py` keys its MOVING classification on and what the
purity tests count. It has moved into its own block; the checker's windows are
path-based, so a collection point on one arm of a diamond is still a collection
point on every path through it.

Unrelated and pre-existing on `main`: three
`gc::tests::runtime_roots::generator_attach_prototype` cases fail identically with
this branch's runtime reverted to `origin/main`. #7731 is the fix.
