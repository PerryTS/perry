// bun:sqlite compatibility over Perry's native SQLite engine (#8510).
import { Database } from "bun:sqlite";

const db = new Database(":memory:", { readwrite: true, create: true });
console.log("filename:", db.filename);

db.run("CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT, payload BLOB)");
const insert = db.query("INSERT INTO items (name, payload) VALUES (?, ?)");
const first = insert.run("alpha", new Uint8Array([1, 2, 3]));
console.log("insert:", first.changes, first.lastInsertRowid);

const named = db.prepare("INSERT INTO items (name, payload) VALUES ($name, $payload)");
named.run({ $name: "beta", $payload: null });

const select = db.query("SELECT id, name, payload FROM items ORDER BY id");
console.log("rows:", select.all().length, select.get()!.name);
const values = select.values();
console.log("values:", values[0][0], values[0][1], values[0][2].length);
select.safeIntegers(true);
console.log("bigint:", typeof select.get()!.id, String(select.get()!.id));

const add = db.transaction((prefix: string, name: string) => {
  const value = prefix + name;
  db.run("INSERT INTO items (name) VALUES (?)", value);
  return value;
});
console.log("transaction:", add("g", "amma"), db.query("SELECT COUNT(*) AS n FROM items").get()!.n);

try {
  db.transaction(() => {
    db.run("INSERT INTO items (name) VALUES ('rolled back')");
    throw new Error("rollback");
  })();
} catch {}
console.log("rollback:", db.query("SELECT COUNT(*) AS n FROM items").get()!.n);

console.log("serialized:", db.serialize().byteLength > 0);
select.finalize();
db.close();
try {
  select.all();
} catch (error) {
  console.log("closed:", error instanceof Error);
}
