Every way of reaching the `Function` constructor now builds the function the
same way `new Function(...)` does, and an auto-optimized binary keeps the
runtime interpreter whenever the program can reach the constructor.

- #10422: `Function(p, body)`, `Function.apply(null, [...])` and
  `Function.call(null, ...)` with a body built at runtime compiled to a
  function that always threw "new Function() cannot run in an ahead-of-time
  compiled binary". `check_eval_function_call` built that stub for the call
  form while the `new` form already fell through to the #6559 interpreter.
  An unfolded call now lowers to the same `js_function_ctor_from_strings`
  construct (the direct call), or calls the `Function` value (`.apply`,
  `.call`, spread arguments). This is the generate-function 2.3.1 `toFunction`
  shape that every mysql2 3.23.2 row parser uses.
- #10423: the global `Function` value carried the shared no-op thunk, so
  `const F = Function; F(p, body)`, lodash's `var Function = context.Function`,
  `Function.bind(...)` and `module.exports = Function` all returned
  `undefined`. It now has a rest-argument call thunk that runs the
  constructor. `fn.constructor(p, body)` returned the dispatcher's empty-object
  stub because a function receiver never resolved its inherited
  `constructor`; it now calls the value the `fn.constructor` read returns.
- #10424: `js_function_ctor_from_strings` read each argument as a string and
  turned anything else into `""`, so `new Function(['a', 'b'], body)` (lodash
  `_.template`'s import names) lost its parameters. Arguments now go through
  ToString, left to right (rooted first, since `toString` is user code), and a
  Symbol throws TypeError. `new Function(...parts)` passed the spread array as
  one argument; it now lowers to `NewDynamicSpread` on the `Function` value.
  The interpreter also binds a rest parameter (`new Function('...xs', body)`),
  which it used to refuse.
- #10421: the auto-optimize build linked `dyn-eval` only for recorded
  runtime-unknown sites. A known-codegen-library site (find-my-way, ajv,
  fast-json-stringify), an unfoldable constant call, and every value route to
  the constructor compiled against a runtime without the interpreter and threw
  at the first call, while `PERRY_NO_AUTO_OPTIMIZE=1` builds worked. Lowering
  now notes each runtime construction it emits, and a per-module AST pre-scan
  (`pre_scan/function_ctor_reach.rs`) notes value uses: the `Function`
  identifier outside `Function.prototype` / `typeof` / `instanceof` /
  equality, a property named `Function`, `globalThis[key]` with a computed key,
  and `.constructor` of a function or any `x.constructor(...)` call.

Reflective construction of the intrinsic now routes to the from-strings entry
before `js_new_function_construct` allocates an instance, since its argument
buffer is not a GC root.

The integration test `function_apply_dynamic_args_eval_surface` asserted the
always-throwing stub; it now asserts the mysql2 shape builds a working
function.

Validation: gap tests `test_gap_10421_function_ctor_as_value`,
`test_gap_10422_function_call_runtime_body` and
`test_gap_10424_function_ctor_to_string_args` fail on v0.5.1589 and match Node
26.5.1 in both no-auto and auto-optimize builds. Each #10421 shape compiled
alone under auto-optimize prints Node's result; programs that never reach the
constructor keep the interpreter out (hello-world size +4 KB).
