| Rooting selection or implementation | Classification | Target / reason or removal scope |
|---|---|---|
| codegen/helpers.rs set_native_roots_for_target; gc_map.rs architecture/format refusals | PLATFORM-REQUIRED | wasm32 WASI (no native frame walker / statepoint backend); arm64_32 watchOS (ILP32, no map loader); ARM64 Windows (no CONTEXT/frame walker); Android and BSD/bare-metal targets (no native section locator/walker). OpenHarmony reports target_os=linux and keeps statepoints. Other architectures rejected by emitter likewise cannot consume native maps. Native x86_64/aarch64 mapped targets require statepoints. WASI is the runnable exception; arm64_32 remains refused by the emitter. |
| codegen/helpers.rs root_home_size_candidate, DEFAULT_ROOT_HOME_RELOCATIONS, ROOT_HOME_MIN_SAFEPOINTS, root_home_relocation_threshold | NOT | #11960 source-size heuristic, any supported native target. |
| codegen/helpers.rs root_spill_relocation_threshold, DEFAULT_ROOT_SPILL_RELOCATIONS, PERRY_ROOT_SPILL_RELOCATIONS | NOT | #8620 estimated 32M relocation ceiling and override. |
| codegen/helpers.rs maybe_spill_roots_to_shadow_frame; call sites in codegen/{functions,closures,classes,helpers}, stmt/function_decl and other inventory entries | NOT | Per-function requests implement the two heuristics above. |
| inprocess.rs enforce_rs4gc_preflight_budget / enforce_rs4gc_instruction_budget, RewriteBudget::Error, spill diagnostics | NOT | Preflight shadow retry removed. The actual post-RS4GC budget now fails closed, with statepoints retained. |
| native_emit.rs apply_budget_spill_retry and text/native/split retry loops; codegen/mod.rs and linker retry transport | NOT | Post-RS4GC and preflight errors switch rooting per function. |
| function.rs force_shadow_frame, request_shadow_frame_spill, spills_roots_to_shadow_frame | NOT | Per-function override bypasses target-selected statepoints. |
| helpers.rs PERRY_RS4GC, rs4gc_env_override | NOT | User-selected shadow backend on targets with working statepoints. |
| helpers.rs PERRY_SHADOW_STACK, precise_root_analysis_enabled | NOT | Can disable platform roots; native roots currently override it for analysis. |
| helpers.rs PERRY_INLINE_SHADOW_SLOT; expr/shadow_inline.rs | NOT selector; platform implementation retained | Inline vs helper shadow traffic must be selected by platform only. |
| function.rs frame push/pop/reservation; expr/shadow_slot.rs; rooting/temp_root.rs; collectors/shadow_slots.rs; root_reload.rs | PLATFORM-REQUIRED implementation, mixed native analysis | Same analysis feeds both lowerings; actual shadow traffic must remain only for unsupported targets. Native temporary roots/slot analysis are not themselves runtime shadow frames. |
| function/precise_roots.rs and remat.rs | Native implementation | Consumes logical shadow binds into native addrspace(1) roots; reload barrier and rematerialization participate in RS4GC. |
| native_emit.rs optnone sabotage test; inprocess.rs optimized-machine budget / split_emit | NOT optimization escape; removed | optnone remains only a negative correctness test. O0 emission/TRE budget paths and their controls are removed. |
| runtime gc/roots/shadow_stack.rs, roots.rs scanners/reexports; gc/mod.rs; exception/savepoints.rs; abi_trampoline.rs, module_require.rs | PLATFORM-REQUIRED implementation; native savepoint shares no shadow state | TLS shadow state, scan/rewrite and unwind restoration. Native helper shadow callers are removed. Runtime frame/TLS/scanning exports compile only on unsupported platforms or for unit tests. |
| runtime gc/roots/stack_roots.rs with_stack_roots | NOT | Rust callback roots use shadow frames on native x86_64 as well; Rust build does not expose RS4GC roots. Replaced with the existing RuntimeHandleScope/scanner; no new registry. |
| runtime object/class_registry/construct/rooted_arguments.rs | NOT | Shadow bindings removed; stable UnsafeCell words register with the existing runtime-handle scanner. |
| runtime regex/perex_replace_storage.rs | NOT | Shadow bindings removed; stable UnsafeCell words register with the existing runtime-handle scanner. |
| runtime shadow-stack tests and codegen NativeRootsPin::shadow / shadow assertions | Tests, mixed | Platform tests retained; supported-target pin and obsolete heuristic assertions require update. |
| stdlib fetch/lifecycle.rs; ext-streams/lib.rs; ext-http/server/mod.rs | TEST-ONLY | Extended-crate shadow uses are cfg(test) fixtures, not native production callers; they retain platform/test scanner coverage. |
| per-target runtime stack_maps.rs / fp_chain / EH walkers | PLATFORM authority | Establish actual map loading and walking capabilities; no heuristic selection. |

The [cycles follow-up](statepoints-only-12023-cycles.md) records the later pinned-main rebase and performance investigation. Owner performance acceptance remains open.

# Statepoint lowering investigation, #12023

Base: d3098e903ff9e68cec2a900b9baf3143445d195c, qb6 x86_64 Linux,
LLVM 22.1.8. All measurements use separately pinned compiler, runtime and
workspace identities. Detailed lane logs are in /root/claude-lanes/sp-work. The complete pre-change
crates inventory (1,388 source matches) is preserved in
[statepoints-only-12023-inventory.txt](statepoints-only-12023-inventory.txt);
its line numbers refer to the exact base commit.

The issue's slot-nonreuse hypothesis is incomplete. At instruction selection
q200 has 200 eight-byte statepoint spill slots; machine allocation transiently
creates 12,395 frame objects, and final fixup leaves 406, with a 3,304-byte
frame. Slot reuse is already happening. It cannot remove the 120,798 fresh
SSA relocates and their mandatory reloads, nor the further copies created
when allocation resolves their live ranges through branch joins.

| N | native SSA symbol B | current base symbol B | relocates | max per point | final frame B | long-copy instructions / machine instructions |
|---:|---:|---:|---:|---:|---:|---:|
| 25 | 38,250 | 31,991 | 1,973 | 25 | 456 | 0 / 7,465 |
| 50 | 120,452 | 64,512 | 7,698 | 50 | 856 | 5,232 / 21,045 |
| 100 | 492,807 | 130,716 | 30,398 | 100 | 1,672 | 49,404 / 78,201 |
| 200 | 1,799,657 | 263,625 | 120,798 | 200 | 3,304 | 219,514 / 275,452 |

These are the ordinary big symbols, excluding specializations/wrappers.
Native SSA is the same source with the shadow size/catastrophic cutoffs disabled.
The numeric-immediate provenance census found zero proven inert relocates in
ordinary big, and the use census found zero unreferenced relocates. An
unconstrained any seed is not proven numeric merely because this particular
caller passes 1; values read from its constructed objects remain pointer
possible unless a representation proof licenses otherwise.

## Candidates measured

| Candidate, q200 | big B | measurement | verdict |
|---|---:|---:|---|
| Stock SSA statepoints / existing automatic slot reuse | 1,799,657 | whole compile 87.39 user CPU s | quadratic copies |
| Remove reload identity barriers | 1,537,971 | source-level negative control | still quadratic; unsafe across lifetime holes (#9499) |
| GC vregs max=1 | 1,886,672 | machine lowering 36.52 CPU s | fails |
| GC vregs max=16 | 1,706,882 | machine lowering 37.81 CPU s | fails |
| GC vregs max=64 | 1,077,628 | machine lowering 40.72 CPU s | fails |
| GC vregs max=1024 / caller-saved fixup | 256,583 | machine lowering 50.75 CPU s | misses size, retains rewrite fanout |
| Individual explicit native homes | 186,065 | rewrite + optimize + emit 10.47 wall s | linear code, metadata fanout remains |
| Grouped explicit native range | 192,649 | rewrite + optimize + emit 6.73 wall s | selected for production prototype |

Grouped q25/q50/q100/q200: 24,817 / 50,245 / 100,495 / 192,649 B and
0.68 / 1.51 / 3.21 / 6.73 seconds. These were measurement-only prototype objects. Final production correctness and removal receipts follow below.

LLVM 22's SelectionDAG/StatepointLowering.cpp records explicit gc-live
allocas as Direct frame locations and states that the contents can be rewritten.
CodeGen/StackMaps.cpp places those locations after ordinary base/derived pairs.
The Direct location does not carry the alloca's array length. The implementation
therefore puts the allocated-type length in the client-owned statepoint ID and
retains the alloca location as its address authority. LLVM IR optimization
drops gc-live entries without relocation users, so publication happens after
IR optimization, immediately before machine emission.

The compact-map encoder already supports repeat root sets. Retaining the
range descriptor until encoding avoids rebuilding a roots-by-calls matrix;
the full range is encoded once and consecutive identical statepoints refer
to it. Existing derived-pointer pairs remain independent.

The standalone q witnesses, their outputs and the symbol-size ratio are
checked by scripts/check_statepoint_linear_size.py; q200 is capped at
248,858 B, with a 2.5x maximum size increase for each doubling in N.

## Final verification, code commit 00b4e15ee

Measurements below use the normal release profile (thin LTO, one codegen unit), LLVM 22.1.8, and separately pinned source/compiler/runtime arms. The earlier 16-codegen-unit prototypes are not the shipping-profile verdict.

| Witness | Base function B | Statepoints function B | Base compile CPU s | Statepoints compile CPU s | Head compact-map B |
|---|---:|---:|---:|---:|---:|
| q25 | 31,991 | 22,172 | 2.49 | 2.26 | 2,749 |
| q50 | 64,512 | 44,477 | 3.84 | 2.67 | 5,752 |
| q100 | 130,716 | 89,478 | 4.65 | 5.59 | 11,252 |
| q200 | 263,625 | 175,935 | 10.55 | 9.10 | 23,854 |

The q200 limit is 248,858 bytes. The final native stack reservation is 232 / 440 / 840 / 1,640 bytes; longest stack-copy runs are 3 / 3 / 3 / 2 instructions, with zero runs of at least 60 instructions. Base shadow reservations are 664 / 1,256 / 2,456 / 4,856 bytes.

The actual compiler sabotage disables only native home retention, retaining all removals. Function bytes become 38,250 / 120,452 / 492,807 / 1,799,657; compile CPU seconds become 1.88 / 3.29 / 10.14 / 48.61. The persisted machine/map gate fails the q200 bound and every doubling. This proves the gate detects removal of the linear mechanism.

Five interleaved runs use perf instructions:u, a clean environment, CPU affinity, exact Node output comparison and GC diagnostics. CPU/RSS/instructions are medians; full-collection counts list every run.

| Workload | Instructions change | CPU s base → head | RSS KiB base → head | Binary B base → head | Fulls base / head |
|---|---:|---:|---:|---:|---|
| tscwork | -0.105% | 3.13 → 3.20 | 264,444 → 260,416 | 202,438,944 → 192,134,216 | [1, 1, 1, 1, 1] / [1, 1, 1, 1, 1] |
| zodwork | -0.086% | 0.10 → 0.10 | 56,836 → 57,028 | 24,946,512 → 24,758,560 | [0, 0, 0, 0, 0] / [0, 0, 0, 0, 0] |
| qs | -0.591% | 2.74 → 2.74 | 59,132 → 57,488 | 27,694,832 → 27,558,752 | [0, 0, 0, 0, 0] / [0, 0, 0, 0, 0] |
| commander | -0.277% | 0.83 → 0.81 | 51,380 → 53,092 | 34,612,192 → 34,568,688 | [0, 0, 0, 0, 0] / [0, 0, 0, 0, 0] |
| hello | -0.033% | 0.00 → 0.00 | 17,792 → 17,740 | 19,026,240 → 19,020,824 | [0, 0, 0, 0, 0] / [0, 0, 0, 0, 0] |

Micro instruction changes for q25/50/100/200 are −0.523% / −1.028% / −1.771% / −3.343%; RSS decreases at every N and full counts are zero in every run. CPU medians are 0.01 s, below useful timing resolution.

Commander RSS is an acceptance concern: earlier shipping-profile batches changed −3.37%, +0.69%, and +2.39%. No across-the-board RSS improvement is claimed. Copied-object sequences match base exactly. The variant-layout/inline runtime-handle prototype was rejected because its instruction improvement weakened to −0.028% and RSS rose further. The final-source table above is preserved without selecting a favorable batch.

### Correctness and gates

- Serial release runtime: 5,007 passed, 5 ignored. Full codegen library: 1,977 passed, 1 ignored, plus integration/doc tests. Both executable regression integrations pass (linear machine/map size, and retired switches cannot disable native statepoints). fmt and fresh normal-profile gc_call_effects pass.
- GC matrix: all 80 witnesses compile on both arms; 79 pass all four knob sets. The readable-from-pipe witness has the same TypeError at inflate on both arms. No new failure. Knob sets: forced evacuation; forced evacuation plus evacuation/mark verification; moving safepoints plus 64 KiB allocation scheduling; budgeted old reclaim in the instrument build.
- The final q200 manual-collection witness returns 19900 in all four sets; force and force+verification each copy 203 objects. The stable-root witness passes forced/verified native and WASI collection.
- Curated native/WASI and final-source isolated dependency dominance/alloca/stale gates retain their original floors and budgets. Every dominance gate catches 40/40 injected violations. Dependency native: 81 modules, 37,660 checked safepoints, 30,431 live bundles, 205,017 relocates; one unrooted hazard within the existing budget of three, zero stale. Dependency WASI: 8,051 functions, zero dominance/alloca/stale violations. Curated WASI stale uses: zero.
- Full lint: base 26/125 failed, head 25/125 failed; no new failing command. Existing audit inventory/debt and tool/environment failures remain; the refreshed WASI ABI authority removes one baseline failure. Two CI-only checks skip. No gate, floor, budget or allowlist is weakened.

### Platforms and large-function receipt

Native reader targets are Apple LP64 x86_64/aarch64, Linux x86_64/aarch64 (including OpenHarmony), and x86_64 Windows. Platform shadow frames remain on WASI, ARM64 Windows, Android, and other targets without a native section locator/walker. arm64_32 watchOS is already refused by the emitter; it is not a runnable backend.

The rebuilt final WASI runtime passes six smoke programs and forced evacuation plus evacuation/mark verification on the stable-root witness (30 checked objects). Root-heavy cross-objects succeed for Android arm64/x86_64 and ARM64 Windows with platform shadow calls; OpenHarmony arm64/x86_64 objects have statepoints and zero shadow calls. Android runtime cross-check with the verified NDK fails identically on base/head in turnloop 0.1.0-alpha.8 (__errno_location and inotify wd types). Hardware runs on those targets are not claimed.

The extracted real 56,506-byte GW7 component compiles and links: base function 19,580,178 B, head 740,053 B; compile CPU 103.04 → 64.72 s, wall 83.89 → 44.73 s, RSS 2,784,332 → 1,610,904 KiB. External names are inert declarations; this is a compile/size receipt, not an application-run claim.

The full 13.64 MB Claude bundle finishes all 128 LLVM units, then reaches the configured 45-minute timeout during coherent optional runtime/stdlib builds for final linking. It is not a successfully linked full-bundle result. Peak RSS 37,093,476 KiB; total CPU 7,346.49 s.

### Final implementation and remaining acceptance

Long-lived managed slots crossing at least 64 collecting sites use statepoint Direct alloca locations. Iterative SCC/span selection is linear; short roots keep ordinary SSA statepoints. Both are the same native rooting backend. Early aggregation creates one alloca with a leaf empty-asm address use, then publication follows LLVM IR optimization. Compact-map v7 carries a range descriptor; the native scanner rewrites each actual root cell.

All non-platform shadow selectors and implementations are removed, including the size heuristic, 32M relocation ceiling, per-function retry, retired environment switches, machine O0 and TRE escapes. The actual post-RS4GC 1,572,864-instruction budget remains fail-closed for unrelated generated-IR expansion; it never changes rooting or optimization. Warn/off alter diagnostics only. Rust-owned callback cells use the existing RuntimeHandleScope/scanner, without a new registry or latch.

The rooting/removal deliverable and linear target are achieved. Strict performance acceptance remains open: final-source commander RSS rises 3.33%, Zod RSS rises 0.34%, and TS CPU is 3.13 → 3.20 seconds despite −0.105% instructions. TS fulls are one in every base/head run; no TS instruction regression is claimed. Earlier padding-sweep TABLE was reviewed, but no lane sweep is claimed. Timing at the 0.00–0.01-second floor cannot establish a strict CPU decrease for hello/micro rows. Android runtime cross-build and a final linked full bundle are also unproved for the reasons above.


## RSS follow-up and completed full-bundle retry

The original table above is retained as the historical five-pair receipt. The follow-up [RSS audit](statepoints-only-12023-rss.md) records 15 pairs with THP on/off, file-backed residency attribution, equal-cache controls, rejected linker prototypes, and the successful full-bundle retry. All equal-cache separate-arm RSS medians are now at or below base; strict CPU acceptance remains unresolved. No compiler/runtime/linker change was promoted in the follow-up.
