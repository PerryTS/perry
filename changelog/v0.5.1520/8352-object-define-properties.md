Fixed `Object.defineProperties` and the descriptor form of `Object.create` to
box primitive property bags, preserve enumerable symbol keys, and observe a
Proxy's single `ownKeys` result in specification order.
