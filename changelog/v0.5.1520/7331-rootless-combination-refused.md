`PERRY_SHADOW_STACK=0` combined with `PERRY_STATEPOINTS=1` (or `PERRY_RS4GC=1`)
produced a binary with **no precise frame roots at all**, silently: no
`__perry_gcmap` section, the same size as a plain shadow-off build, and it ran
and printed the correct answer. Nothing distinguished it from a correct build
until a collection moved something live.

The cause is structural rather than a missing check. The statepoint backends are
an alternative *lowering* of the shadow stack's root-set analysis, not an
independent mechanism: `reserve_shadow_slot()` is the single entry point that,
under `native_stack_roots_enabled()`, allocates a stack-map slot instead — and
the caller of that analysis returns empty maps outright when the shadow stack is
off. Switching one off switches the other off with it.

The combination is now a hard error. Each knob alone is unchanged, so the
bisection knob keeps its meaning; it simply cannot be combined with a backend
that depends on the analysis it disables.

Worth recording for the adoption plan: because the two share this analysis,
"delete the shadow stack and keep statepoints" is not currently expressible, and
any plan treating them as interchangeable mechanisms needs that premise
corrected first.
