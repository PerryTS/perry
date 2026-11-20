// parity-node-argv: --experimental-sqlite
// node:sqlite StatementSync parameter binding conformance (#6561):
// anonymous, named ($/:/@), bare vs prefixed keys, unknown-named errors,
// mixed named+anonymous, and the setAllow* toggles.
import { DatabaseSync } from "node:sqlite";

function row(value) {
  if (value === undefined) return "undefined";
  return JSON.stringify(value);
}

function report(label, fn) {
  try {
    console.log(label, "OK", fn());
  } catch (error) {
    console.log(label, "THROW", `${error.name}:${error.code}:${String(error.message).split("\n")[0]}`);
  }
}

const db = new DatabaseSync(":memory:");
db.exec("CREATE TABLE t(a, b)");
const clear = () => db.exec("DELETE FROM t");
const first = () => row(db.prepare("SELECT a, b FROM t LIMIT 1").get());

report("anonymous", () => {
  db.prepare("INSERT INTO t VALUES (?, ?)").run(1, "one");
  return first();
});
report("named dollar bare keys", () => {
  clear();
  db.prepare("INSERT INTO t VALUES ($x, $y)").run({ x: 2, y: "two" });
  return first();
});
report("named colon and at", () => {
  clear();
  db.prepare("INSERT INTO t VALUES (:x, @y)").run({ x: 3, y: "three" });
  return first();
});
report("named prefixed keys", () => {
  clear();
  db.prepare("INSERT INTO t VALUES ($x, $y)").run({ $x: 4, $y: "four" });
  return first();
});
report("named mixed with anonymous", () => {
  clear();
  db.prepare("INSERT INTO t VALUES ($x, ?)").run({ x: 5 }, "five");
  return first();
});
report("missing trailing anonymous", () => {
  clear();
  db.prepare("INSERT INTO t VALUES (?, ?)").run(6);
  return first();
});
report("too many anonymous", () => {
  db.prepare("INSERT INTO t VALUES (?, ?)").run(1, 2, 3);
  return first();
});
report("unknown named key", () => {
  db.prepare("INSERT INTO t VALUES ($x, $y)").run({ x: 1, y: 2, nope: 3 });
  return first();
});
report("select with named param", () => {
  clear();
  db.prepare("INSERT INTO t VALUES ($x, $y)").run({ x: 7, y: "seven" });
  return row(db.prepare("SELECT b FROM t WHERE a = $x").get({ x: 7 }));
});

report("setAllowBareNamedParameters(false)", () => {
  const stmt = db.prepare("INSERT INTO t VALUES ($x, 'bare')");
  stmt.setAllowBareNamedParameters(false);
  try {
    stmt.run({ x: 8 });
    return "bare accepted";
  } catch (error) {
    return `bare rejected ${error.code}`;
  }
});
report("setAllowUnknownNamedParameters(true)", () => {
  clear();
  const stmt = db.prepare("INSERT INTO t VALUES ($x, 'unk')");
  stmt.setAllowUnknownNamedParameters(true);
  stmt.run({ x: 9, extra: "ignored" });
  return first();
});

db.close();
