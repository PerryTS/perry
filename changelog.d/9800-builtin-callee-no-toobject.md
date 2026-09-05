### Runtime

- fix(runtime): a method call on a primitive whose callee is a BUILT-IN no
  longer boxes the receiver. ECMA-262 §10.3.1: a built-in function's `[[Call]]`
  does not run `OrdinaryCallBindThis` — it receives `thisArg` unchanged and
  performs its own coercion, which every `String`/`Number`/`Boolean`/`BigInt`
  prototype thunk already does (they accept the raw primitive before looking
  for a wrapper payload). Perry boxed for them anyway, and for a string
  receiver that `ToObject` wrapper materialises an own index property per
  UTF-16 code unit. Only a sloppy USER callee is owed the wrapper now, which is
  the distinction the spec draws.
