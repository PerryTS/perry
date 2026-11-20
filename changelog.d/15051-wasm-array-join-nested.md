`--target web`: `join` converts elements as `Array.prototype.join` does, so `[[1, 2], [3]].join(";")` is `1,2;3` and `null` / `undefined` elements are empty (#15035).
