Fixed a 75× property-store cliff on `const o = {}; const X = 1; o.a = X;`
(#10353). The straight-line builder fold (#6812) rewrites `const o = {}`
plus its following `o.k = v` assignments into the object literal they spell
out, which is what gives the object a closed anon shape, a shape-stamped
allocation and direct stores. It only matched when the assignments followed
the binding *immediately*, so a single ordinary declaration in between — the
usual way initialisation code names its constants — dropped the whole
sequence back onto the dynamic `js_put_value_set` path, where every store
re-interns and re-coerces the key and transitions the object's shape. The
same program with the value passed as a parameter, or with the constants
written inline, was 75× faster, which is what made the cliff look like a
property of the stored *value*.

`fold_builder_sequences` now skips up to 64 statements between an **empty**
`{}` binding and its first assignment, sinking the allocation below them. A
statement is skippable only when moving the allocation past it is
unobservable, which is the pair of conditions the value side already carries
(`gap_stmt_is_hoistable`): it must not name the binding, and it must not be
able to execute user code — a call can reach a hoisted
`function peek() { return o; }` that names the binding without the statement
naming it, which would turn a successful read into a TDZ `ReferenceError`.
Destructuring patterns (getter-bearing property reads) and populated
literals are excluded; sinking `const o = { a: y }` below `const y = 1`
would hide a TDZ throw. Skipped statements keep their relative order and
still run before every folded value.

Measured with `perf stat -e instructions:u` on x86_64, 2400 iterations
building a six-property object with `--no-auto-optimize`: 108,447,339 →
1,399,772 instructions (77×), matching the same program with the constants
written inline (1,401,872) or the value passed as a parameter (1,411,774).
Nothing that folded before folds differently — the gap is an additional
match, and a statement that fails the test leaves the original dynamic
writes exactly as they were: `benchmarks/object-write-6812` and the
`bench_*` corpus move by at most 0.006%, and a 12k-line file whose gaps
never reach an assignment (maximum pre-scan work, zero folds) costs 0.019%
more to compile. A file where the fold now applies compiles 51% cheaper,
because 1,200 dynamic store sites become 200 stamped allocations.
