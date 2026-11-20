Async generators are born on one shared record layout, like sync generators:
the instance carries a private `[[GeneratorState]]` slot and a brand, its
prototype is held by the record, and the request queue lives in the record.
`next`/`return`/`throw` are the `%AsyncGeneratorPrototype%` methods, which
check the brand and enqueue on the record, instead of three closures owned by
every instance. The `async_generator_queue` side table (`STATES`) and its GC
root scan are deleted: a suspended generator stays alive exactly while its
pending step or a queued request is reachable, and an abandoned one is
collected. The async body is one step closure with an operation parameter.

Async generator class methods (named, symbol-keyed and computed) are now
async generator functions: their own `prototype` and their instances chain to
`%AsyncGeneratorPrototype%`. `return E` in an async generator awaits `E`
inside its enclosing `try`, so a returned rejected promise is caught by the
generator's own `catch`, as in node.

Gap test `test_gap_async_generator_record` covers request queueing, return and
throw at every state, delegation, thenables, microtask order, methods, brands
and liveness; runtime tests cover GC liveness of suspended and abandoned
generators.
