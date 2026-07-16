// parity-node-argv: --experimental-sqlite
import { DatabaseSync } from "node:sqlite";

const db = new DatabaseSync(":memory:");
db.exec("CREATE TABLE items(id INTEGER PRIMARY KEY, name TEXT)");
const sql = db.createTagStore(10);
console.log(
  "shape:",
  typeof sql.run,
  typeof sql.get,
  typeof sql.all,
  typeof sql.iterate,
  typeof sql.clear,
);
console.log("identity:", sql.db === db, sql.capacity, sql.size);

console.log(
  "run 1:",
  String(sql.run`INSERT INTO items(name) VALUES (${"alpha"})`.changes),
  sql.size,
);
console.log(
  "run 2:",
  String(sql.run`INSERT INTO items(name) VALUES (${"beta"})`.changes),
  sql.size,
);
const first: any = sql.get`SELECT id, name FROM items WHERE name = ${"alpha"}`;
console.log(
  "get:",
  first.id,
  first.name,
  Object.getPrototypeOf(first) === null,
  sql.size,
);
const all: any[] = sql.all`SELECT id, name FROM items ORDER BY id`;
console.log("all:", all.length, all.map((row) => row.name).join(","), sql.size);
const iterated = [...sql.iterate`SELECT name FROM items ORDER BY id`].map(
  (row: any) => row.name,
);
console.log("iterate:", iterated.join(","), sql.size);
console.log("missing:", String(sql.get`SELECT * FROM items WHERE id = ${99}`));

sql.clear();
console.log("cleared:", sql.size, sql.capacity);
db.close();
