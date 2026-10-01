Codegen now answers "does this local hold a Number here?" with one query over
one scoped set: the function-wide number-by-construction locals plus the locals
a guarded loop clone admitted at its entry. The per-loop-family accumulator
lists are gone, and the answer now also reaches the raw-double store and add
paths, so a reassigned Number accumulator such as `h = h + o.a` no longer takes
a value check on every use.
