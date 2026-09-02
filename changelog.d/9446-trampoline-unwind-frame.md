**The all-f64 call trampoline is now unwindable while its callee runs**
(#9446) — the deterministic `PERRY_GC_SCHEDULE_SEED=1 PERRY_GC_SCHEDULE_RATE=1`
SIGSEGV on the Claude Code bundle at safepoint 4266, and a latent root-loss
and lost-`catch` defect behind it on x86-64.

`abi_trampoline::call_all_f64` (`perry-runtime/src/abi_trampoline.rs`) is how
every dynamic vtable dispatch (`class_registry::dispatch::call_vtable_method`:
by-name method calls, bound methods, `new` on a class value) invokes a callee
whose arity is only known at runtime. With more than eight f64 arguments —
`this` counts, and a synthesized capture-stashing constructor in a bundle has
dozens — it lowers the stack pointer by a runtime amount to spill the rest and
leaves it there across the call. It did that inside an `asm!` block in an
ordinary Rust function, so the frame description LLVM emitted for that function
never knew: measured on x86-64 Linux, the FDE says `CFA = rsp+32` at the `call`
while `rsp` is really `stack_bytes` lower.

Every unwinder that steps through the trampoline while the callee runs then
reads the trampoline's return address from the wrong slot — a spilled argument
or a saved register:

- **The GC's native-root walk** (`gc/roots/stack_maps.rs`, an
  `_Unwind_Backtrace`) stops at the trampoline when the garbage is a mapped
  address, silently dropping every frame ABOVE it from the root set for that
  collection: a young object the caller holds across the call is not copied,
  the caller reads a recycled cell later, and nothing names the collection.
  When the garbage is not mapped, libgcc's fallback frame probe dereferences it
  and the collector dies inside `_Unwind_Backtrace` — which is #9446's crash:
  a minor at a loop poll inside `new LoggerProvider(…)` (OpenTelemetry's class
  expression, replayed through the constructor table with its capture params),
  `rax = 0xa` at `cmpb $0x48,(%rax)`.
- **The exception transport** (`_Unwind_RaiseException`, the system unwinder
  on x86-64): a `throw` inside such a callee never finds the `catch` above the
  trampoline and is reported as uncaught.

aarch64 never showed either: LLVM happened to keep a frame pointer for the
trampoline function there, so the CFA was `x29`-relative and immune to `sp`.

Both trampolines are now **naked functions** (`#[unsafe(naked)]` /
`naked_asm!`) that set `rbp` / `x29` from the entry stack pointer before
anything moves, define the CFA off that register with their own
`.cfi_startproc … .cfi_endproc` region, and drop the spill area through it after
the call. The dynamic adjustment is then invisible to unwinding on every target
and under every frame-pointer setting, and the frame record is what a
frame-pointer chain walk expects too. Argument marshalling is unchanged
(`split_register_and_stacked` feeds the same eight register slots and the
16-byte-rounded spill area the inline version built).

Validation:

- `abi_trampoline::tests::unwind_through_the_trampoline::the_unwinder_steps_through_a_trampoline_with_stacked_args`
  walks the stack with `_Unwind_Backtrace` from inside a 12-argument callee
  (four stacked on both ABIs) and requires the frames above the caller to be
  the same ones the caller's own walk sees. On the old trampolines it dies with
  SIGSEGV on x86-64 Linux — the collector's crash, in a unit test.
- `test-files/test_gap_9446_trampoline_unwind.ts` is the JS-level witness with
  no GC knobs: a throw through a 9-parameter dynamically dispatched method, a
  throw through a 9-parameter class-expression constructor, and a nursery
  collection inside a 9-parameter dynamically dispatched method while the
  caller holds a young object.
- Claude Code (`cli_2.1.112.js`) under `PERRY_GC_SCHEDULE_SEED=1
  PERRY_GC_SCHEDULE_RATE=1` no longer dies at safepoint 4266.

The issue's three unexamined leads, examined: `PERRY_GC_VERIFY_MARK`'s
`marked->UNMARKED edges` on a copying minor are old-generation children that a
MINOR legitimately leaves unmarked (a known false positive of that verifier,
see #8770's ladder); `PERRY_GC_FROMSPACE_SCAN`'s `type=5 (Promise) +0 bare`
offenders are the seven padding bytes after the one-byte `state` field, which
`ptr::write(promise, Promise::new())` leaves holding recycled arena bits and
which no rewrite descriptor ever reads (also a known scanner false positive);
`PERRY_CONSERVATIVE_STACK_SCAN=1`'s segfault was not chased here (see the PR
for what it does on the fixed binary).
