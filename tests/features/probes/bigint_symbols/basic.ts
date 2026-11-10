const key = Symbol.for("perry");
const value = 2n ** 5n;

console.log(`bigint-symbol:${String(key)}:${value}`);
