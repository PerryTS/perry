// better-sqlite3 `stmt.run()` accepts a row-returning statement: it steps the
// statement to completion, discards the rows, and reports the connection's
// counters. rusqlite's `Connection::execute` refuses one with
// `ExecuteReturnedResults`, and the error was swallowed into a null return, so
// every `run()` on a SELECT arrived in JS as `undefined` with no error raised.
//
// ActiveRecord's sqlite3 adapter hits this through `step()`, which calls
// `run()` on `SELECT sqlite_version(*)` and then reads `.changes` off the
// result — failing as `Cannot read properties of undefined (reading 'changes')`.
import Database from "better-sqlite3";

const db = new Database(":memory:");
db.exec(`CREATE TABLE t (id INTEGER PRIMARY KEY AUTOINCREMENT, v TEXT)`);

// Row-returning statements: run() must report a result, not null/undefined.
// Note `typeof null` is "object", so these assert the value, not its type.
const sel = db.prepare("SELECT 1 AS one");
const selRun = sel.run();
console.log("select result=", JSON.stringify(selRun));
console.log("select changes=", selRun.changes);
console.log("select lastInsertRowid=", selRun.lastInsertRowid);

const ver = db.prepare("SELECT sqlite_version(*)");
console.log("version result=", JSON.stringify(ver.run()));

// The same statement still reads rows through get()/all().
console.log("select get().one=", (db.prepare("SELECT 1 AS one").get() as { one: number }).one);

// Non-row-returning statements keep their existing behaviour.
const ins = db.prepare("INSERT INTO t (v) VALUES ('x')");
const insRun = ins.run();
console.log("insert changes=", insRun.changes);
console.log("insert lastInsertRowid=", insRun.lastInsertRowid);

// `changes` is per-statement: a CREATE TABLE straight after an INSERT
// reports 0, not the 1 left in the connection's running counter.
const ddl = db.prepare("CREATE TABLE u (id INTEGER)");
console.log("ddl result=", JSON.stringify(ddl.run()));

// Bound parameters on both kinds.
const insB = db.prepare("INSERT INTO t (v) VALUES (?)");
console.log("bound insert changes=", insB.run("y").changes);
const selB = db.prepare("SELECT v FROM t WHERE v = ?");
console.log("bound select result=", JSON.stringify(selB.run("y")));

// Rows are really consumed, so the table reflects both inserts.
console.log("count=", (db.prepare("SELECT COUNT(*) AS n FROM t").get() as { n: number }).n);
