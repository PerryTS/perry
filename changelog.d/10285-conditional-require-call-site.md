Fixed CommonJS `require()` calls inside branches, ternaries, short-circuit
operands, logical assignments, `try` blocks, `switch` cases and loop bodies
being hoisted into eager imports. The required module now initializes when the
`require` executes, as in Node, instead of before the requiring module's first
statement — and not at all when the branch is never taken. Deferred targets
initialize through the path-module registry, so side-effect-only modules run,
a throwing `require` stays inside its `try`/`catch`, and deferred classes get
their static fields.
