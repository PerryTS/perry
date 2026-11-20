// parity-node-argv: --experimental-sqlite
import { DatabaseSync } from "node:sqlite";

const db = new DatabaseSync(":memory:");
const row: any = db
  .prepare(
    "SELECT 1 AS __proto__, 2 AS constructor, 3 AS toString, 4 AS hasOwnProperty",
  )
  .get();
console.log("prototype:", Object.getPrototypeOf(row) === null);
console.log("keys:", Object.keys(row).join(","));
console.log(
  "values:",
  row.__proto__,
  row.constructor,
  row.toString,
  row.hasOwnProperty,
);
console.log(
  "own:",
  Object.prototype.hasOwnProperty.call(row, "__proto__"),
  Object.prototype.hasOwnProperty.call(row, "constructor"),
  Object.prototype.hasOwnProperty.call(row, "toString"),
  Object.prototype.hasOwnProperty.call(row, "hasOwnProperty"),
);

const arrays = db
  .prepare("SELECT 5 AS __proto__, 6 AS constructor", { returnArrays: true })
  .get() as any[];
console.log("array values:", arrays.join(","));
db.close();
