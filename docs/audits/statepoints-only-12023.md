| Rooting selection or implementation | Classification | Target / reason or removal scope |
|---|---|---|
| codegen/helpers.rs set_native_roots_for_target; gc_map.rs architecture/format refusals | PLATFORM-REQUIRED | wasm32 WASI (no native frame walker / statepoint backend); arm64_32 watchOS (ILP32, no map loader); ARM64 Windows (no CONTEXT/frame walker). Other architectures rejected by emitter likewise cannot consume native maps. Verify reachable target set before removal. |
| codegen/helpers.rs root_home_size_candidate, DEFAULT_ROOT_HOME_RELOCATIONS, ROOT_HOME_MIN_SAFEPOINTS, root_home_relocation_threshold | NOT | #11960 source-size heuristic, any supported native target. |
| codegen/helpers.rs root_spill_relocation_threshold, DEFAULT_ROOT_SPILL_RELOCATIONS, PERRY_ROOT_SPILL_RELOCATIONS | NOT | #8620 estimated 32M relocation ceiling and override. |
| codegen/helpers.rs maybe_spill_roots_to_shadow_frame; call sites in codegen/{functions,closures,classes,helpers}, stmt/function_decl and other inventory entries | NOT | Per-function requests implement the two heuristics above. |
| inprocess.rs enforce_rs4gc_preflight_budget / enforce_rs4gc_instruction_budget, RewriteBudget::Spill, spill diagnostics | NOT | Constructed-IR budget asks for shadow retry. |
| native_emit.rs apply_budget_spill_retry and text/native/split retry loops; codegen/mod.rs and linker retry transport | NOT | Post-RS4GC and preflight errors switch rooting per function. |
| function.rs force_shadow_frame, request_shadow_frame_spill, spills_roots_to_shadow_frame | NOT | Per-function override bypasses target-selected statepoints. |
| helpers.rs PERRY_RS4GC, rs4gc_env_override | NOT | User-selected shadow backend on targets with working statepoints. |
| helpers.rs PERRY_SHADOW_STACK, precise_root_analysis_enabled | NOT | Can disable platform roots; native roots currently override it for analysis. |
| helpers.rs PERRY_INLINE_SHADOW_SLOT; expr/shadow_inline.rs | NOT selector; platform implementation retained | Inline vs helper shadow traffic must be selected by platform only. |
| function.rs frame push/pop/reservation; expr/shadow_slot.rs; rooting/temp_root.rs; collectors/shadow_slots.rs; root_reload.rs | PLATFORM-REQUIRED implementation, mixed native analysis | Same analysis feeds both lowerings; actual shadow traffic must remain only for unsupported targets. Native temporary roots/slot analysis are not themselves runtime shadow frames. |
| function/precise_roots.rs and remat.rs | Native implementation | Consumes logical shadow binds into native addrspace(1) roots; reload barrier and rematerialization participate in RS4GC. |
| native_emit.rs optnone sabotage test; inprocess.rs optimized-machine budget / split_emit | NOT shadow path currently | optnone test proves hidden roots unsafe. O0 final emission retains statepoints; inventory must distinguish optimization escape from rooting escape. |
| runtime gc/roots/shadow_stack.rs, roots.rs scanners/reexports; gc/mod.rs; exception/savepoints.rs; abi_trampoline.rs, module_require.rs | PLATFORM implementation plus shared native-runtime callers | TLS shadow state, scan/rewrite and unwind restoration. Cannot classify as platform-only until native helper callers removed. |
| runtime gc/roots/stack_roots.rs with_stack_roots | NOT | Rust callback roots use shadow frames on native x86_64 as well; Rust build does not expose RS4GC roots. Must replace shared runtime callers if removal is reached. |
| runtime object/class_registry/construct/rooted_arguments.rs | NOT | Native runtime constructor arguments bind stack cells to shadow frame. |
| runtime regex/perex_replace_storage.rs | NOT | Native replacer callbacks bind argument and storage cells to shadow frames. |
| runtime shadow-stack tests and codegen NativeRootsPin::shadow / shadow assertions | Tests, mixed | Platform tests retained; supported-target pin and obsolete heuristic assertions require update. |
| per-target runtime stack_maps.rs / fp_chain / EH walkers | PLATFORM authority | Establish actual map loading and walking capabilities; no heuristic selection. |

# Statepoint lowering investigation, #12023

Base: d3098e903ff9e68cec2a900b9baf3143445d195c, qb6 x86_64 Linux,
LLVM 22.1.8. All measurements use separately pinned compiler, runtime and
workspace identities. Detailed lane logs are in /root/claude-lanes/sp-work.

The issue's slot-nonreuse hypothesis is incomplete. At instruction selection
q200 has 200 eight-byte statepoint spill slots; machine allocation transiently
creates 12,395 frame objects, and final fixup leaves 406, with a 3,304-byte
frame. Slot reuse is already happening. It cannot remove the 120,798 fresh
SSA relocates and their mandatory reloads, nor the further copies created
when allocation resolves their live ranges through branch joins.

| N | native SSA symbol B | current base symbol B | relocates | max per point | final frame B | long-copy instructions / machine instructions |
|---:|---:|---:|---:|---:|---:|---:|
| 25 | 39,433 | 33,745 | 1,973 | 25 | 456 | 0 / 7,465 |
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
0.68 / 1.51 / 3.21 / 6.73 seconds. These are measurement-only objects,
not a correctness claim. The production implementation, gates and GC knob
matrix must pass before the non-platform shadow mechanisms are removed.

LLVM 22's SelectionDAG/StatepointLowering.cpp records explicit gc-live
allocas as Direct frame locations and states that the contents can be rewritten.
CodeGen/StackMaps.cpp places those locations after ordinary base/derived pairs.
The Direct location does not carry the alloca's array length. The prototype
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
