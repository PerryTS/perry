### Delete the explicit statepoint bridge — one native-root backend, not two

Perry carried two statepoint backends. The **explicit bridge** rewrote Perry's
own IR text into `gc.statepoint` calls with hand-emitted relocations; **RS4GC**
retypes root allocas and lets LLVM's `RewriteStatepointsForGC` insert every
statepoint and relocation itself. The bridge is gone.

They were never peers. RS4GC does strictly more: the bridge **cannot root an
`invoke`**, so since #7330 it refused try-carrying functions outright, and CI
had to skip `09_try_catch_roots` on that arm. Keeping a mode that cannot
compile what its sibling compiles — plus its textual emitter, its call parser,
and its knob — is the permanent hybrid this project keeps paying for.

The bridge was also RS4GC's fallback: a bail in the RS4GC recognizer silently
downgraded the whole function to it. **Measured before removing it: 1,574
functions across `test-drizzle-pg` (1,543) and the gc-ratchet probes (31) all
lowered as `rs4gc`, none fell back.** A fallback nothing takes is an untested
configuration, which is what the GC knob kill-policy exists to prevent — so a
bail is now a hard failure naming the function, not a silent downgrade.

Deleted with it, because only the bridge used them: the CFG-based root-liveness
analysis (RS4GC gets liveness from LLVM's SSA form), the direct-call parser and
statepoint emitter, the `PreciseRootBackend` enum, and the `PERRY_STATEPOINTS`
knob — `PERRY_RS4GC=1` is now the single switch, and one fewer GC knob is one
less kill-policy debt. `native_stack_roots_enabled()` is just `rs4gc_enabled()`.

Net **−1,216 lines**. The default shadow-stack path is untouched.

Verified: all ten gc-ratchet probes byte-match the pinned Node oracle on the
sole backend under `PERRY_GC_FORCE_EVACUATE=1 PERRY_GC_VERIFY_EVACUATION=1
PERRY_STACKMAP_WALKER=verify`, the default arm is 10/10, `test-drizzle-pg`
still builds, and 593 codegen unit tests pass.
