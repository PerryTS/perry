// parity-node-argv: --experimental-sqlite
// node:sqlite user-defined functions and aggregates (#6561), including the
// dynamically-dispatched (any-typed receiver, closure context) paths that
// previously fell through perry's method tower and silently no-op'd.
import { DatabaseSync } from "node:sqlite";

function report(label, fn) {
  try {
    console.log(label, "OK", fn());
  } catch (error) {
    console.log(label, "THROW", `${error.name}:${error.code}:${String(error.message).split("\n")[0]}`);
  }
}

report("function simple", () => {
  const db = new DatabaseSync(":memory:");
  db.function("double_it", (x: any) => x * 2);
  const v = (db.prepare("SELECT double_it(21) AS v").get() as any).v;
  db.close();
  return v;
});

report("function varargs option", () => {
  const db = new DatabaseSync(":memory:");
  db.function("addall", { varargs: true }, (...args: number[]) =>
    args.reduce((acc, value) => acc + value, 0)
  );
  const v = (db.prepare("SELECT addall(1,2,3,4) AS v").get() as any).v;
  db.close();
  return v;
});

report("function deterministic option", () => {
  const db = new DatabaseSync(":memory:");
  db.function("three", { deterministic: true }, () => 3);
  const v = (db.prepare("SELECT three() AS v").get() as any).v;
  db.close();
  return v;
});

report("aggregate", () => {
  const db = new DatabaseSync(":memory:");
  db.exec("CREATE TABLE t(a); INSERT INTO t VALUES (1),(2),(3)");
  db.aggregate("sumsq", {
    start: 0,
    step: (acc: number, value: number) => acc + value * value,
  });
  const v = (db.prepare("SELECT sumsq(a) AS v FROM t").get() as any).v;
  db.close();
  return v;
});

report("aggregate with result callback", () => {
  const db = new DatabaseSync(":memory:");
  db.exec("CREATE TABLE t(a); INSERT INTO t VALUES (2),(4)");
  db.aggregate("avg2", {
    start: () => ({ sum: 0, count: 0 } as any),
    step: (acc: any, value: number) => ({ sum: acc.sum + value, count: acc.count + 1 }),
    result: (acc: any) => (acc.count === 0 ? null : acc.sum / acc.count),
  });
  const v = (db.prepare("SELECT avg2(a) AS v FROM t").get() as any).v;
  db.close();
  return v;
});

// Regression: any-typed receiver inside a closure goes through the dynamic
// method tower; `function`/`aggregate` were missing from its name gate.
report("function via any-typed receiver", () => {
  const db = new DatabaseSync(":memory:");
  (db as any).function("addall2", { varargs: true }, (...args: number[]) =>
    args.reduce((acc, value) => acc + value, 0)
  );
  const v = (db.prepare("SELECT addall2(5,6) AS v").get() as any).v;
  db.close();
  return v;
});

report("aggregate via any-typed receiver", () => {
  const db = new DatabaseSync(":memory:");
  db.exec("CREATE TABLE t(a); INSERT INTO t VALUES (1),(2)");
  (db as any).aggregate("sumsq2", {
    start: 0,
    step: (acc: number, value: number) => acc + value * value,
  });
  const v = (db.prepare("SELECT sumsq2(a) AS v FROM t").get() as any).v;
  db.close();
  return v;
});

report("function receives sqlite values", () => {
  const db = new DatabaseSync(":memory:");
  db.exec("CREATE TABLE t(a INTEGER, b TEXT, c REAL)");
  db.exec("INSERT INTO t VALUES (7, 'x', 1.5)");
  db.function("kinds", { varargs: true }, (...args: any[]) =>
    args.map((value) => `${typeof value}:${String(value)}`).join("|")
  );
  const v = (db.prepare("SELECT kinds(a, b, c, NULL) AS v FROM t").get() as any).v;
  db.close();
  return v;
});
