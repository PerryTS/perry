// parity-node-argv: --experimental-sqlite
import { DatabaseSync } from "node:sqlite";

const db = new DatabaseSync(":memory:");
const sqliteType = Symbol.for("sqlite-type");
console.log("value:", (db as any)[sqliteType]);
console.log("own:", Object.prototype.hasOwnProperty.call(db, sqliteType));
console.log(
  "symbol key:",
  Object.getOwnPropertySymbols(db).includes(sqliteType),
);
db.close();
