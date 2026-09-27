**perf(codegen): leaf-mark audited non-collecting helpers on `invoke`, not only `call` (#11500).** `lower_roots_for_rs4gc` appends `"gc-leaf-function"` to direct calls whose callee `gc_call_effects::classify_direct_callee` proves `CannotCollect` (or `AllocNoReentry` under the safepoint-only contract), but it only recognised `call` / `tail call` lines. Inside a `try` the same helper is emitted as `invoke … to label %cont unwind label %pad`, so it was never marked, and RS4GC turned every such site into a full statepoint that relocates every live GC value. On one large real-world bundle, 164k of 291k `js_closure_get_capture_bits` sites were these unmarked invokes.

The invoke arm (`leaf_marked_invoke` in `crates/perry-codegen/src/function/precise_roots.rs`) uses the same classification as the call arm. The only textual difference is where the attribute goes. On an invoke, function attributes sit after the argument list and before `to label`, which is the placement `call_gc_leaf`, the #8596 transitive annotator, and the native C-API reader (`dialect/eh.rs`) already use. A site that already carries a call-site attribute is left alone, so nothing is marked twice. Inline asm is marked for the call arm's reason: RS4GC cannot wrap asm in a statepoint.

Measured on a small try-heavy probe (two closure factories whose captured bindings are read and written inside `try`/`catch`/`finally`). The `--trace llvm` output was run through stock `opt` 22.1.8 with the shipped `always-inline,function(mem2reg,sccp),rewrite-statepoints-for-gc` pipeline:

| metric | before | after |
|---|---:|---:|
| `gc.statepoint` sites | 229 | 182 |
| of which `invoke` statepoints | 116 | 69 |
| `gc.relocate` | 534 | 352 |

The 49 newly marked invoke sites are `js_write_barrier_root_nanbox` (15), `js_closure_get_capture_bits` (9), `js_box_set_bits` (9), `js_write_barrier` (9) and `js_string_addref_if_heap_string` (7). Collecting callees in the same `try` bodies are still statepoints.

Tests: `audited_accessor_invoke_inside_try_takes_no_statepoint` builds a function the way `lower_try` does (personality, active EH scope, landing pad) and runs it through the in-process RS4GC pipeline. The accessor invoke must stay a direct invoke, and an unaudited `js_map_alloc` invoke in the same body must stay an `invoke token … gc.statepoint`. `invoke_leaf_marking_places_the_attribute_before_the_successors` pins the placement, the no-double-mark rule, the untouched control, and the asm case at the text level. Both tests were sabotage-checked in both directions: disabling the invoke arm turns both red, and marking every invoke turns both red through the collecting control.
