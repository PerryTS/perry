### Native GC roots work on x86-64

`PERRY_RS4GC=1` was refused on anything but aarch64 (#7324). The refusal was
correct — the runtime could not resolve x86-64 frame bases, and the collector
segfaulted rather than reporting anything — but the cause was one unsupported
call, not anything architectural.

On x86-64 every root is `Indirect [RSP + off]`, DWARF register 7. The unwinder
path resolved bases with `_Unwind_GetGR(context, reg)`, and **`_Unwind_GetGR`
is not a supported query for the stack-pointer column** — it returned garbage
that the collector then wrote through. `_Unwind_GetCFA` is the supported way.

SP-relative roots now derive their base from the CFA: by the SysV/AAPCS
definition it is the caller's stack pointer immediately before the call, so the
body stack pointer sits one return-address slot plus this function's own frame
below it, and `stack_size` is exactly that frame, already recorded per function
in the map. The architecture's SP register number is a **runtime-local**
constant, deliberately separate from the format's base tags — those stay
aarch64-literal so the compiler's idea of the target and the runtime's
`target_arch` can never disagree (see `gc_map.rs`).

Measured on real x86-64 Linux hardware: **all ten gc-ratchet probes byte-match
the pinned Node oracle** under `PERRY_RS4GC=1 PERRY_GC_FORCE_EVACUATE=1
PERRY_GC_VERIFY_EVACUATION=1`, with `.perry_gcmap` present in every binary.

The walker demonstrably ran rather than passing vacuously — root-source
telemetry on x86-64 reports `walks=1, frames_visited=10, records_matched=1,
locations_visited=2` on `02_survivor_promotion`, with `fp_walks=0` and
`fallback_walks=1` (the fp-chain walker is aarch64-only, so the unwinder is the
correct path there), and the evacuation moved objects
(`retained_forwarded_stub_objects=5`).

**The same telemetry on aarch64 has the same shape** (`7/0/1/1` vs `10/0/1/1`
on the same probes, `2` locations on `09_try_catch_roots` on both), which is
what makes this an equivalence result rather than a green light of unknown
provenance.

Caveat worth recording: those location counts are low **on both platforms**.
The probes end in an explicit `gc()`, a manual collection from a shallow stack,
so precise native roots are lightly exercised by this suite regardless of
architecture. That is a pre-existing gate weakness, not something this change
introduces, and it means "x86-64 matches aarch64" is a stronger claim here than
"x86-64 is heavily exercised".
