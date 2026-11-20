### Native-frame GC roots via LLVM statepoints, opt-in (#7173, #7174)

Adds a second precise-root mechanism alongside the shadow stack, selected by
`PERRY_STATEPOINTS=1` (explicit bridge) or `PERRY_RS4GC=1` (LLVM's
`RewriteStatepointsForGC` owns statepoint and relocation insertion). Either
one activates native roots on its own — `native_stack_roots_enabled()` is
`statepoints || rs4gc` — so `PERRY_RS4GC=1` does not require
`PERRY_STATEPOINTS=1`. **The default path is unchanged**: with neither set
nothing here runs, and the shadow stack remains the shipping root mechanism.

The point of the mechanism is that the forgot-to-root bug class becomes
structurally impossible — LLVM, not Perry, is responsible for knowing which
values are live across a call and for rewriting them after a collection moves
them.

**Every root path fails closed.** The plain `llvm.experimental.stackmap`
lowering is deleted outright rather than kept as a fallback: LLVM may record a
root slot's address as `Register R#N`, caller-saved and unrecoverable at
collection time, so a fallback to it silently loses roots. It survived in three
places, all of which failed open:

* `PreciseRootBackend::StackMap` was dead by construction (both sites setting
  `stack_map_requested` are guarded by `native_stack_roots_enabled()`, which is
  exactly `statepoints || rs4gc`);
* the statepoint backend fell back for calls it could not parse — chiefly
  **indirect** calls. That was a limitation of Perry's textual parser, not of
  statepoints: `gc.statepoint` takes its callee as a `ptr` operand, so
  `ptr elementtype(T) %fnptr` is as valid as `... @callee`. Indirect targets are
  now statepoint-able and anything still unparseable is a hard compile error;
* the compact-map rewriter fell back to keeping LLVM's section, which reads as
  conservative and is not — the runtime reads only `__perry_gcmap`, so those
  records sit unread and that module's roots go missing. Now a hard error.

**The metadata is re-encoded rather than shipped as LLVM emits it.** Measured on
`test-drizzle-pg`, `__llvm_stackmaps` was 4.21 MB, of which >50% was data the
runtime already discarded at startup: three `Constant` slots per record
(`gc.statepoint`'s calling-convention preamble) and a duplicate of every root
(LLVM records base and derived; Perry has no interior pointers). Perry now
rewrites that block at assembly time — where LLVM prints the function addresses
as symbol names, so one text parser replaces Mach-O *and* ELF relocation
parsing plus a second link pass — into a compact map: 4,214,384 B → 224,832 B
(19.0× measured same-build; 18.5× once the conservative `js_throw`
classification below is accounted for). The largest single lever is that **77% of records have the identical
live set as the record before them**, so a repeat flag replaces the payload;
that also lets the runtime share one copy per distinct set instead of
materialising 154k entries.

**Try/catch is covered.** Now that exception lowering uses `invoke`/`landingpad`
(#7302), no jump can skip a `gc.relocate`, so try-carrying functions take
statepoints like any other. Under RS4GC they additionally need
`landingpad token` — RS4GC uses the landing pad *as* the relocate token — which
is sound here only because the pad's value is dead; the retype refuses if the
pad register is referenced anywhere.

`benchmarks/gc_ratchet/probes/09_try_catch_roots.ts` is new and exists because
nothing in the suite contained a `try` at all: objects allocated inside a `try`
surviving a collection there, locals live across a throw and read in the
`catch`, a throw crossing several frames so the rewritten roots sit in a
caller's frame, `finally` on both edges, and a rethrow caught one frame up.

**Measured on `test-drizzle-pg` (133 modules):** 23,301 safepoints, all
statepoints, 0 plain stack maps, 0 parser fallbacks, 129,914 relocations.
Binary size is a wash against the shadow stack (+496 B for the bridge,
+50,064 B for RS4GC): statepoints generate less code (`__text` −151 KB / −240 KB,
plus ~105 KB less `__eh_frame`) and that is cancelled by the remaining
189–221 KB of map. Runtime is −0.93% (RS4GC) and RSS is flat.

Also lands three mode-independent codegen fixes that the work depended on:
codegen-unit globals are emitted only into units that reference them and
declarations are scoped to the unit that needs them (per-unit IR previously grew
with unit *count*, which is why the 13 MB `@anthropic-ai/claude-code` bundle hit
`clang: translation unit is too large` no matter how finely it was split —
885 KB → 299 KB per unit on a 4-unit module), and codegen units now compile with
bounded parallelism (`PERRY_CODEGEN_UNIT_JOBS`, default `parallelism/4` clamped
to `[1,4]`) instead of one at a time.
