// Gap test for node:sqlite DatabaseSync / StatementSync core surface.
// Closes #3183 (DatabaseSync lifecycle) and #3184 (StatementSync
// metadata and result modes). Byte-for-byte parity target:
//   node --experimental-strip-types test_gap_sqlite_3183plus.ts
// Deterministic: uses an in-memory (:memory:) database only.
import { DatabaseSync } from "node:sqlite";

const db = new DatabaseSync(":memory:");

// DatabaseSync instance is an object (#3183).
console.log("db typeof:", typeof db);

// exec() runs DDL and returns undefined.
const execRet = db.exec(
  "CREATE TABLE people (id INTEGER PRIMARY KEY, name TEXT, age INTEGER)",
);
console.log("exec ret:", execRet);

// prepare().run() returns { changes, lastInsertRowid } (#3184).
const ins = db.prepare("INSERT INTO people (name, age) VALUES (?, ?)");
console.log("stmt typeof:", typeof ins);

const r1 = ins.run("alice", 30);
console.log("r1 changes:", r1.changes, "rowid:", r1.lastInsertRowid);
const r2 = ins.run("bob", 25);
console.log("r2 changes:", r2.changes, "rowid:", r2.lastInsertRowid);
const r3 = ins.run("carol", 41);
console.log("r3 changes:", r3.changes, "rowid:", r3.lastInsertRowid);

// all() returns an array of row objects keyed by column name (#3184).
const sel = db.prepare("SELECT id, name, age FROM people ORDER BY id");
const rows = sel.all();
console.log("rows length:", rows.length);
console.log("row0:", rows[0].id, rows[0].name, rows[0].age);
console.log("row1:", rows[1].id, rows[1].name, rows[1].age);
console.log("row2:", rows[2].id, rows[2].name, rows[2].age);

// get() returns a single row object (or undefined).
const one = db.prepare("SELECT name, age FROM people WHERE id = ?").get(2);
console.log("get name:", one.name, "age:", one.age);

const miss = db.prepare("SELECT name FROM people WHERE id = ?").get(999);
console.log("get miss:", miss);

// Bound-parameter aggregate.
const cnt = db.prepare("SELECT COUNT(*) AS n FROM people WHERE age > ?").get(26);
console.log("count > 26:", cnt.n);

// columns() metadata shape (#3184): name + type populated.
const colsStmt = db.prepare("SELECT id, name FROM people");
const cols = colsStmt.columns();
console.log("cols length:", cols.length);
console.log("col0 name:", cols[0].name, "type:", cols[0].type);
console.log("col1 name:", cols[1].name, "type:", cols[1].type);

// close() — lifecycle teardown (#3183).
db.close();
console.log("closed");
