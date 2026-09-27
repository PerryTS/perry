### Runtime

- fix(runtime): a method call on a primitive whose callee is a BUILT-IN no
  longer boxes the receiver (#11509, re-land of #9800's `32f150b22`). ECMA-262
  §10.3.1: a built-in function's `[[Call]]` does not run
  `OrdinaryCallBindThis`. It receives `thisArg` unchanged and does its own
  coercion, which every `String`/`Number`/`Boolean`/`BigInt`/`Object`
  prototype thunk already does (they accept the raw primitive before looking
  for a wrapper payload). `call_primitive_closure_value` boxed for them
  anyway. For a string receiver, that `ToObject` wrapper materialises an own
  index property per UTF-16 code unit. Only a sloppy USER callee gets the
  wrapper now, which is the distinction the spec draws.

  Unlike the original commit, built-in-ness is not read from the per-instance
  `builtin_closure_length` side table. It is a new `BUILTIN` kind bit on the
  closure BODY record (`closure/registry.rs`, next to `STRICT` and #10521's
  `NON_CONSTRUCTOR`), set once per thunk by `install_proto_method`,
  `install_proto_method_rest_with_length` and
  `primitive_proto_method_closure_value`. The call site checks
  `STRICT | BUILTIN` in the same single body-record lookup it already made for
  strictness. The bit is a positive mark, so any body not registered as a
  built-in keeps the old wrapper behaviour. No new table, and no
  per-closure entries for the collector to prune.
