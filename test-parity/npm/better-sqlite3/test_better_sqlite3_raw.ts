// Refs #643 — better-sqlite3 stmt.raw() / stmt.raw().all() / stmt.raw().get().
// drizzle's `PreparedQuery.values()` calls `this.stmt.raw().all(...params)`
// to get back row arrays; without `.raw()` the chain dies as
// `(number).all is not a function` deeper down.
import Database from "better-sqlite3";

const sqlite = new Database(":memory:");
sqlite.exec(`CREATE TABLE t (id INTEGER, name TEXT)`);
sqlite.exec(`INSERT INTO t VALUES (1, 'alice'), (2, 'bob')`);

// Object mode (default) — rows are objects keyed by column name.
const stmt1 = sqlite.prepare("SELECT * FROM t");
const objRows = stmt1.all();
console.log("objRows.length=", objRows.length);
console.log("objRows[0].name=", objRows[0].name);

// Raw mode via direct call site — rows are arrays of column values.
const stmt2 = sqlite.prepare("SELECT * FROM t");
const rawRows: any = (stmt2 as any).raw().all();
console.log("rawRows.length=", rawRows.length);
console.log("rawRows[0][0]=", rawRows[0][0]); // id
console.log("rawRows[0][1]=", rawRows[0][1]); // name

// Raw mode via chained call returns the same handle (chains for .get).
const stmt3 = sqlite.prepare("SELECT * FROM t WHERE id = ?");
const rawRow: any = (stmt3 as any).raw().get(1);
console.log("rawRow[0]=", rawRow[0]);
console.log("rawRow[1]=", rawRow[1]);
