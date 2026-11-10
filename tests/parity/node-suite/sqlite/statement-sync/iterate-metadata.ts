// parity-node-argv: --experimental-sqlite
// node:sqlite StatementSync iterate() protocol + statement metadata
// conformance (#6561): for..of, manual next() with `value: null` on done,
// return() termination, sourceSQL/expandedSQL, columns().
import { DatabaseSync } from "node:sqlite";

function report(label, fn) {
  try {
    console.log(label, "OK", fn());
  } catch (error) {
    console.log(label, "THROW", `${error.name}:${error.code}:${String(error.message).split("\n")[0]}`);
  }
}

const db = new DatabaseSync(":memory:");
db.exec("CREATE TABLE t(a INTEGER, b TEXT)");
db.exec("INSERT INTO t VALUES (1,'one'),(2,'two'),(3,'three')");

report("for..of", () => {
  const seen: string[] = [];
  for (const row of db.prepare("SELECT a FROM t ORDER BY a").iterate() as any) {
    seen.push(String(row.a));
  }
  return seen.join(",");
});
report("iterate with param", () => {
  const seen: string[] = [];
  for (const row of db.prepare("SELECT a FROM t WHERE a > ? ORDER BY a").iterate(1) as any) {
    seen.push(String(row.a));
  }
  return seen.join(",");
});
report("manual next / done value", () => {
  // Note: next() after the done result is deliberately NOT probed — Node's
  // lazy iterator resets the statement on exhaustion and a further next()
  // restarts the query, a quirk of its lazy stepping.
  const it: any = db.prepare("SELECT a FROM t ORDER BY a LIMIT 1").iterate();
  const r1 = it.next();
  const r2 = it.next();
  return [
    `done=${r1.done} a=${r1.value?.a}`,
    `done=${r2.done} value=${String(r2.value)}`,
  ].join(" | ");
});
report("return() terminates", () => {
  const it: any = db.prepare("SELECT a FROM t ORDER BY a").iterate();
  const first = it.next();
  const ret = it.return();
  const after = it.next();
  return `first=${first.value?.a} ret done=${ret.done} value=${String(ret.value)} after done=${after.done} value=${String(after.value)}`;
});
report("break in for..of", () => {
  let count = 0;
  for (const _row of db.prepare("SELECT a FROM t").iterate() as any) {
    count += 1;
    if (count === 2) break;
  }
  return count;
});

report("sourceSQL", () => db.prepare("SELECT a FROM t WHERE a = ?").sourceSQL);
report("expandedSQL", () => {
  const stmt = db.prepare("SELECT a FROM t WHERE a = ?");
  stmt.get(2);
  return stmt.expandedSQL;
});
report("columns()", () => {
  const cols = (db.prepare("SELECT a, b AS bee, 1 + 1 AS calc FROM t") as any).columns();
  return JSON.stringify(cols);
});

report("statement after close finalized", () => {
  const tmp = new DatabaseSync(":memory:");
  tmp.exec("CREATE TABLE x(a)");
  const stmt = tmp.prepare("SELECT * FROM x");
  tmp.close();
  return stmt.get();
});

db.close();
