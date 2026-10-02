Complete the remaining one-shape compiler paths: shape-record lookup reads the
published per-agent directory directly; immutable method slots use shape-owned
ConstFn metadata with worker transfer and unload handling. Cyclic module startup
prepares literal pools and closed literal layouts before eager bodies execute.

Numeric receiver regions use shape and F64 field proofs for loop arithmetic and
individual comparisons, preserving evaluation order, exception handlers and
generic fallback. Read-only regions also accept classless runtime records without
granting store permission; virtual namespace and per-object reads remain excluded.
Delete the separate numeric class-field loop emitter and its cached raw-pointer
facts, retaining ordinary property fallbacks and specialized array/element paths.

Keep receivers, assignment results and dynamic-add operands rooted across
collecting fallbacks. Correct synchronous IteratorClose ordering and nested catch
completion handling: return operands evaluate before close, cleanup runs once,
and exceptions bypass catches already exited by the pending completion.

Add compiler, runtime and executable regression coverage for numeric route
admission and refusal, closure identity, worker metadata, moving collections,
cyclic initialization, operand ordering and nested iterator cleanup.

Worker executable fixtures allocate and recheck captures across scheduled
collections; the fixed seeds retain positive-copying and moved-object assertions.

Worker programs keep the 32 cached results of each literal-prefix string concatenation in thread-local cells. Each worker now owns and roots its own strings, so repeated worker launches and simultaneous workers cannot reuse another agent's cached heap handles. The single-agent cache and its checked miss/root-registration path are preserved. Added graph-level IR ownership coverage and an executable two-launch regression that checks exact concatenation results.

Fix synchronous for-of IteratorClose when an inner break or continue from finally cancels a pending return. Captured exits restore the completion inherited at their target, so cleanup loops retain an outer pending return and normal iterator exhaustion does not call return(). Preserve generated preludes around labeled control targets. Recognize every label in a label chain as targeting its terminal loop when deciding which finally blocks a captured exit crosses.

Preserve normal call boundaries for closure-bearing inline candidates inside loops that perform receiver field arithmetic. This keeps closure creation out of the body that guarded numeric regions must version, while retaining codegen's refusal to duplicate closures and its recheck after calls. Tiny callees, calls outside these loops, and loops without receiver field arithmetic continue to inline. Focused tests cover all loop forms, nested helper inlining, and both controls.

Run the try/catch native-root probe on Windows after its exception lowering moved to landing pads. Require Node-equivalent execution, a native root map, nonzero evacuation and RS4GC root records; keep the linker refusal for actual WinEH funclet IR. Use the runtime TLS declaration macro for worker launch test observations.

Reuse cached key-add transitions for an exact safe ConstFn body when its traced
Any intermediate is still live. Store the current receiver's closure with the
ordinary barrier before publishing its body-specific shape, using one existing
cache probe. Missing intermediates, deprecated facts and unsupported targets
retain the rooted slow publication path. Regressions cover distinct captures,
actual moving collections, pointer-key fallback and publication ordering.
