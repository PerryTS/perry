Made strict equality against a proven `Symbol()` or `Symbol.for()` value use
direct identity instead of the generic JavaScript equality helper. The proof
comes only from a stable constructor initializer, never from an erased
TypeScript `symbol` annotation; reassigned bindings and loose equality retain
their semantic runtime paths.

This removes two generic `js_eq` calls from codehz/ecs's 10k-entity
accumulation loop. On an Apple M1 Mac mini, 11 alternating process pairs
measured the row at 0.195845 ms versus 0.615553 ms on the parent change, a
68.193% median paired improvement with 11/11 wins. The full ECS suite remained
7/7 with checksum 50005000, and a forced verified-GC Symbol stress run recorded
87 copying minors and 11,470 copied objects with Node-identical output.
