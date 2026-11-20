Consolidates the GC correctness effort into a single plan, in
`docs/src/internals/rfc-rooting-by-construction.md`, rather than five documents
and issues that each describe part of the same shape.

States the defect shape once, then splits it by *where the untracked pointer
lives* — codegen's lowering code (this RFC), the emitted machine code's liveness
(statepoints, #7108/#7174), and hand-written runtime Rust (#7231/#7249) — and
records that in-process LLVM (#7241) is the enabler for the second, since #7108
found the text-IR-plus-stock-clang architecture is what rules the cheap design
out.

Also records that adopting statepoints *deletes* three of this RFC's stated
"cannot catch" entries rather than mitigating them, because the shadow frame
stops existing.
