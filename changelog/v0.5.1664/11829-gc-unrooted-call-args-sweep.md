Fixed the rest of the #11789 bug class: a call that lowered its arguments into
bare registers and held an earlier heap argument across a later argument that
collects, so the call received a pre-move (retired from-space) address. The
closure-call fix covered one lowering; this sweep moves every other
argument-lowering loop in `lower_call/` and the call-like HIR lowerings in
`expr/` onto the shared rooted-group helpers (`lower_call_args_rooted`,
`lower_operands_rooted`, `lower_operand_list_rooted`): `fs.*` natives, the
`node:util` / `process` / `crypto` / WebCrypto helpers, direct `js_*` runtime
calls, native-module instance methods (the receiver is now rooted across its
arguments), `perry/ui` / `perry/tui` / `perry/system` tables (a `Str` argument
is converted to its raw pointer only after every argument has been lowered),
static / `super` / `super(...)` calls, promise `.then` / `.catch`, `new
Headers` / `Response` / `Blob` / `File` / `ReadableStream` / `WritableStream` /
`TransformStream` option literals, and the FFI-manifest call path. Calls whose
later arguments cannot collect emit the IR they emitted before. New seeded-GC
witnesses `test_gap_gc_11789_native_call_args`,
`test_gap_gc_11789_ctor_call_args` and `test_gap_gc_11789_dispatch_call_args`,
plus the IR-level `args_sweep_tests`.
