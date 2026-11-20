// parity-node-argv: --experimental-sqlite
import { DatabaseSync, StatementSync } from "node:sqlite";

function proto(row: any) {
  return row === undefined
    ? "undefined"
    : Object.getPrototypeOf(row) === null
      ? "null"
      : "other";
}

const db = new DatabaseSync(":memory:");
console.log("StatementSync:", typeof StatementSync, StatementSync.length);
try {
  new (StatementSync as any)();
  console.log("direct construct: OK");
} catch (error: any) {
  console.log("direct construct: THROW", error.name, error.code || "no-code");
}

console.log(
  "exec return:",
  String(
    db.exec(
      "CREATE TABLE items(id INTEGER PRIMARY KEY, name TEXT, score REAL)",
    ),
  ),
);
const insert = db.prepare("INSERT INTO items(name, score) VALUES (?, ?)");
console.log("insert 1:", JSON.stringify(insert.run("alpha", 1.5)));
console.log("insert 2:", JSON.stringify(insert.run("beta", 2.5)));

const select = db.prepare("SELECT id, name, score FROM items ORDER BY id");
const first: any = select.get();
console.log("get:", proto(first), first.id, first.name, first.score);
const all: any[] = select.all();
console.log(
  "all:",
  all.length,
  proto(all[0]),
  all.map((row) => row.name).join(","),
);
console.log(
  "empty get:",
  String(db.prepare("SELECT * FROM items WHERE id = -1").get()),
);
console.log(
  "empty all:",
  db.prepare("SELECT * FROM items WHERE id = -1").all().length,
);
db.close();
