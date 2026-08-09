Fixed spread calls on node-core module namespaces, which passed the spread
operand as ONE argument holding the whole array instead of expanding it
(#7720). `path.join(...parts)` threw `TypeError [ERR_INVALID_ARG_TYPE]` where
the identical non-spread call succeeded; the same positional fold made
`util.format(...args)` inspect its array instead of formatting it and
`fs.existsSync(...args)` test an array for existence.

Every native-module fast path in `lower_call` consumes its arguments
positionally, so `path.join(...parts)` reached the one-argument arm as
`PathNormalize(<array>)`. When any argument is spread and the callee is a
node-core module namespace method (or a named export of one), `lower_call` now
declines the whole fast-path chain; the fall-through tail builds an
`Expr::CallSpread` over the namespace member — the lowering the value-read form
(`const j = path.join; j(...parts)`) already took — which materialises the
argument array and dispatches through `js_native_call_method` →
`dispatch_native_module_method`. That dispatcher is variadic by construction, so
it gets both the valid case and Node's `ERR_INVALID_ARG_TYPE` (with its `code`)
for an invalid one right. Generalises the per-module bail #6668 added for
`crypto`, whose two guards stay because they also cover the bare `crypto`
GLOBAL receiver.

Deliberately scoped to node-core modules: an ext/npm native module (mysql2,
redis, node-forge) has no by-name runtime dispatcher behind its codegen-wired
`NativeMethodCall` rows, so declining its fast path would trade a wrong answer
for no answer. Native class statics (`Buffer.concat`, `URL.parse`) are excluded
for the same reason — `Buffer.concat` is already broken through the dynamic
path for the plain non-spread call — and both exclusions are asserted by tests.

`crates/perry-hir/src/lower/expr_call/native_module_spread_tests.rs` asserts the
verdict in both directions: a spread call is diverted, and a non-spread
`path.join('a','b')` still lowers to `PathJoin`. The second half matters because
the generic dispatch is a correct fallback, so a regression that disabled the
fast path everywhere would still print the right answer. It is also why
`node-suite/path/join/type-errors-extra.ts` has spread-called `path.join` since
long before this fix and stayed green throughout — it only spreads segments Node
rejects, so both the broken and the correct lowering threw. Behaviour is
byte-compared against node in new `node-suite/path/join/spread.ts`,
`node-suite/path/resolve/spread.ts` and `node-suite/util/format/spread.ts`;
`--suite node-suite --module path` is 94/94.
