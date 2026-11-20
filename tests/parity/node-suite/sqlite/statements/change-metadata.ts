// parity-node-argv: --experimental-sqlite
import { DatabaseSync } from "node:sqlite";

function result(label: string, value: any) {
  console.log(
    label,
    typeof value.changes,
    String(value.changes),
    typeof value.lastInsertRowid,
    String(value.lastInsertRowid),
  );
}

const db = new DatabaseSync(":memory:");
result(
  "create:",
  db.prepare("CREATE TABLE data(id INTEGER PRIMARY KEY, value TEXT)").run(),
);
const insert = db.prepare("INSERT INTO data(value) VALUES (?)");
result("insert one:", insert.run("a"));
result("insert two:", insert.run("b"));
result("update:", db.prepare("UPDATE data SET value = upper(value)").run());
result("delete:", db.prepare("DELETE FROM data WHERE id = ?").run(1));
result("select:", db.prepare("SELECT * FROM data").run());
console.log(
  "remaining:",
  db.prepare("SELECT id, value FROM data").get().id,
  db.prepare("SELECT value FROM data").get().value,
);
db.close();
