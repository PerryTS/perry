**test: pin the `for…of` desugar counter's literal kind at both emission sites (#7766 follow-up).**

#7778 changed both `for…of` desugar sites to mint the synthetic `__idx_*`
counter as `Expr::Integer(0)` instead of `Expr::Number(0.0)` — the literal
KIND `collect_integer_let_ids` seeds on, and therefore the difference between
a counter that gets a canonical i32 slot and one that is structurally
invisible to every i32-counter loop optimization (the element-shape clone's
matcher hard-requires `ctx.i32_counter_slots`).

Nothing pinned it. The desugar is correct either way and prints identical
output, so a future edit back to `Number(0.0)` would silently un-optimize
every `for…of` loop with no test failing — CLAUDE.md's fourth way a gate can
be unable to fail. These are verdict tests on the lowered HIR, one per
emission site (module-init and function-body, which have drifted
independently before), and both are sabotage-verified: reverting either site
to `Number(0.0)` reds them.
