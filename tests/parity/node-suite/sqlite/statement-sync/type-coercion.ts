// parity-node-argv: --experimental-sqlite
// node:sqlite JS<->SQLite type coercion conformance (#6561): null/number/
// bigint/string/Uint8Array binding, boolean/undefined rejection, numbers
// always bind as REAL, INTEGER reads come back as plain numbers (typeof
// "number"), safe-integer overflow throws, setReadBigInts round-trips.
import { DatabaseSync } from "node:sqlite";

function describe(value) {
  if (value === null) return "null";
  if (value === undefined) return "undefined";
  if (value instanceof Uint8Array) return `u8[${Array.from(value).join(",")}]`;
  return `${typeof value}:${String(value)}`;
}

function report(label, fn) {
  try {
    console.log(label, "OK", fn());
  } catch (error) {
    console.log(label, "THROW", `${error.name}:${error.code}:${String(error.message).split("\n")[0]}`);
  }
}

const db = new DatabaseSync(":memory:");
db.exec("CREATE TABLE t(v)"); // no affinity: stored types reflect the binding

report("bind null", () => {
  db.prepare("INSERT INTO t VALUES (?)").run(null);
  return describe((db.prepare("SELECT v FROM t LIMIT 1").get() as any).v);
});
report("numbers bind as REAL", () => {
  db.exec("DELETE FROM t");
  db.prepare("INSERT INTO t VALUES (?)").run(5);
  db.prepare("INSERT INTO t VALUES (?)").run(5.5);
  const rows = db.prepare("SELECT v, typeof(v) AS ty FROM t").all() as any[];
  return rows.map((r) => `${describe(r.v)}/${r.ty}`).join(" ");
});
report("bigint binds as INTEGER", () => {
  db.exec("DELETE FROM t");
  db.prepare("INSERT INTO t VALUES (?)").run(123n);
  const r = db.prepare("SELECT v, typeof(v) AS ty FROM t").get() as any;
  return `${describe(r.v)}/${r.ty}`;
});
report("string round-trip", () => {
  db.exec("DELETE FROM t");
  db.prepare("INSERT INTO t VALUES (?)").run("hello");
  return describe((db.prepare("SELECT v FROM t").get() as any).v);
});
report("blob round-trip", () => {
  db.exec("DELETE FROM t");
  const buf = new Uint8Array([0, 255, 128, 7]);
  db.prepare("INSERT INTO t VALUES (?)").run(buf);
  const v = (db.prepare("SELECT v FROM t").get() as any).v;
  return `${describe(v)} isU8=${v instanceof Uint8Array}`;
});
report("bind boolean rejected", () => db.prepare("INSERT INTO t VALUES (?)").run(true as any));
report("bind undefined rejected", () => db.prepare("INSERT INTO t VALUES (?)").run(undefined as any));
report("huge bigint rejected", () => db.prepare("INSERT INTO t VALUES (?)").run(2n ** 70n));

report("run() result typeofs", () => {
  db.exec("DELETE FROM t");
  const r = db.prepare("INSERT INTO t VALUES (?)").run("x");
  return `changes=${describe(r.changes)} lastInsertRowid=${describe(r.lastInsertRowid)}`;
});

const intdb = new DatabaseSync(":memory:");
intdb.exec("CREATE TABLE i(v INTEGER)");
intdb.prepare("INSERT INTO i VALUES (?)").run(9007199254740993n);
report("unsafe INTEGER read throws", () => intdb.prepare("SELECT v FROM i").get());
report("setReadBigInts(true)", () => {
  const stmt = intdb.prepare("SELECT v FROM i");
  stmt.setReadBigInts(true);
  const big = describe((stmt.get() as any).v);
  stmt.setReadBigInts(false);
  return big;
});
intdb.close();
db.close();
