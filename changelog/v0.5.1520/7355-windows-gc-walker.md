`PERRY_RS4GC=1` native GC roots now work on x86-64 Windows (#7354): a new
`RtlVirtualUnwind`-based stack walker in `gc/roots/stack_maps.rs` steps native
frames and enumerates `.pgcmap` root slots, so the COFF refusal in
`compact_and_assemble` is lifted for `x86_64-pc-windows-msvc` (ARM64 Windows
stays refused — it still has no walker). Verified on a real Windows host: 9/9
runnable gc-ratchet probes byte-match the pinned Node oracle under forced
evacuation + evacuation verification, with walker telemetry live (probe 04:
5,626 frames visited, 5,449 records matched). `gc-native-roots.yml` gains a
`windows-latest` arm with a `--require-locations` telemetry liveness gate.

Two Windows-only compile hazards found on the way are now handled fail-closed:
the unguarded `eh_walker` calls in `exception.rs` that broke the whole Windows
build of perry-runtime are cfg-gated, and RS4GC refuses modules whose `try`
lowered to WinEH funclet pads *before* piping them to LLVM —
`rewrite-statepoints-for-gc` crashes outright on funclet EH (access violation
in opt 22.1.3, eight-line upstream repro in the PR). A failed RS4GC opt
pipeline now also writes its input IR to disk with a one-command repro line
instead of leaving only a symbol-less stack dump.
