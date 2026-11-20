Argument-shape clone routes keep their runtime class+ShapeId guard whenever the caller's proof came
from the barrier-bypassing route-only pass, and a clone that publishes its parameter no longer keeps
a caller-side containment fact at all.

The route-only proof deliberately bypasses rule 5's module-wide §5.2 shape-barrier kill
(`collectors/ptr_shape.rs`), which is the belt-and-braces backstop against blind spots in the
containment walk. Pairing that bypass with an elided entry guard, and simultaneously dropping the
requirement that the callee preserve containment, removed every net that could observe a reshaped
argument: a module carrying an unattributable `Object.defineProperty`/`delete`/`Proxy` site, a
method that publishes its parameter after its licensed read, and two call sites on the same caller
local produced two unguarded direct calls into `…$pshape_args`, the second reading declared fields
at fixed offsets from an object the compiler itself had recorded as published to an alias it cannot
see.

`PrefixContainedParamUse` proves a temporal property — the licensed reads happen *before* the body
publishes the parameter — but the fact map that carries a caller-side route is keyed by local id and
is therefore flow-insensitive, so a fact kept past a publishing call is consulted again at every
later route site for that local. A per-local map cannot express "before", so route admission now
requires the clone to preserve containment for the parameter's whole lifetime, and the
`require_post_call_containment` knob whose only other mode was unsound is deleted rather than left
selectable.

Guard elision is retained for exactly the case that justifies it: a caller holding the broad
`Ptr<Shape>` representation fact, which by construction was proven in a barrier-free module under
full containment, where the caller is already licensed to read the same object's declared fields at
fixed offsets and the guard is tautological. The measured `perform-ecs` and Wolf routes are
unaffected — they are all fresh contained locals in barrier-free modules — and the #8774 slice
carried no speed claim to begin with.

Pinned by `published_argument_in_a_barrier_module_never_reaches_an_unguarded_clone`, which asserts
its own subject is live (the clone must still be emitted) and fails with "2 clone calls, 0 guard
blocks" against the pre-fix code.
