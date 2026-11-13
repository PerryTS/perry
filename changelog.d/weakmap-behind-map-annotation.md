A WeakMap or WeakSet behind a `Map` / `Set` annotation runs its own methods again (`m.get(k)` threw
`value is not a function` since the folded Map / Set sites test their receiver). The Map / Set calls the compiler
leaves unfolded (`this.k.get(x)`, `this.k.forEach(f)`, `keys()` / `values()` / `entries()`), a folded `clear()` and
the `size` read now test the receiver's kind as well: any other value (an Array, a plain object with its own `get`,
a WeakMap, a Set behind `Map<K, V>`) gets the ordinary property lookup and call, as in node, instead of the Map
built-in answering `undefined` / `false` / `0`. `PERRY_COLLECTION_UNFOLDED_TESTS=0` turns the new tests off.
