`stmt_is_packed_f64_loop_safe` rejected every `throw` in a packed-f64 loop
body, on the reasoning that the thrown value is typically constructed
(`throw new Error(…)`) and the construction is a call in the loop. That is
true of the construction, not of the `throw` — a loop that throws an
already-built value lost the fast path for a call it never makes.

`contains_gc_unsafe_call` now answers `false` for a block whose last
instruction is `unreachable`. That is sound for the reason the whole
call-free check exists: the check verifies at compile time that nothing in
the fast clone can trigger a collection and move the preheader-cached
receiver *before something reads it again*. A block ending in `unreachable`
has no such "again" — control does not return to the loop, so no cached value
is read after the call. `push_inst` drops anything pushed after a terminator,
so a block's last instruction is its terminator; `instructions.last()` being
`Unreachable` therefore means the block genuinely ends there rather than
carrying dead code past a `br`. A `throw` that constructs its value is still
rejected, because the construction is emitted in blocks that precede the
terminating one.

`Stmt::Try` inside the loop body remains a rejection, so the throw's handler
can never be inside the loop being cloned — which is what would otherwise
return control to a loop holding a stale cached pointer.

bench: 4.76 → 0.58 ns.
