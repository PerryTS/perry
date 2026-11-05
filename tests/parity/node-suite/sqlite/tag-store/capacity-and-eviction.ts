// parity-node-argv: --experimental-sqlite
import { DatabaseSync } from "node:sqlite";

const db = new DatabaseSync(":memory:");
const small = db.createTagStore(2);
console.log("small initial:", small.capacity, small.size);
console.log("one:", (small.get`SELECT ${1} AS value` as any).value, small.size);
console.log(
  "two:",
  (small.get`SELECT ${2} AS value, 2 AS marker` as any).value,
  small.size,
);
console.log(
  "one cached:",
  (small.get`SELECT ${3} AS value` as any).value,
  small.size,
);
console.log(
  "three evicts:",
  (small.get`SELECT ${4} AS value, 3 AS marker` as any).value,
  small.size,
);
console.log(
  "two again:",
  (small.get`SELECT ${5} AS value, 2 AS marker` as any).value,
  small.size,
);

const zero = db.createTagStore(0);
console.log(
  "zero:",
  zero.capacity,
  zero.size,
  (zero.get`SELECT ${6} AS value` as any).value,
  zero.size,
);
const defaults = db.createTagStore();
console.log("default:", defaults.capacity, defaults.size);
db.close();
