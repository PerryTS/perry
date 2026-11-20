**perf(codegen): elide the length computation of a statement-position push**

`arr.push(x)` evaluates to the new length, computed by `js_array_length` —
which is not a field read: it resolves Proxy arrays through the `get` trap and
probes the registered-Set/Map side tables. A statement-position `arr.push(x);`
discards that result, so push-heavy code paid an out-of-line runtime call per
push for a number nobody reads.

The elision is gated on the `mem::take`n per-expression signal from
`dispatch::lower_expr` (#7590/#7591), which reaches exactly the statement's own
expression and never an operand — `n = arr.push(x)` and every other consuming
position still computes the real length, covered by a consuming-position test
matrix (call-argument / assignment / arithmetic / conditional / nested /
spread / boxed) that is byte-identical to Node.

Measured: 2.77× on a pure statement-position push loop (9.0 → 3.25 ns/push —
the out-of-line call was also blocking loop optimization around it); ~3 % on
the GC-bound `json_pipeline` at 200k records. Verified live in traced IR: the
discarded push emits zero `js_array_length` calls, the consumed push exactly
one per specialization.
