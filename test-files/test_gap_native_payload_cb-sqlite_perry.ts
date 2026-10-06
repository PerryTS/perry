// #11919 perry-only expectations for node:sqlite callbacks (CALLBACK-DESIGN
// T6/T7/T10, LIFECYCLE-DESIGN L7/L9). Node 26.5.1 crashes (SIGSEGV) on
// close() inside a running UDF and never collects a database whose UDF
// captures it, so this file is checked against
// test-parity/expected/test_gap_native_payload_cb-sqlite_perry.txt instead of
// node.
import { DatabaseSync } from "node:sqlite";

const db = new DatabaseSync(":memory:");
db.exec("create table t(a integer)");
const ins = db.prepare("insert into t values(?)");
for (let i = 0; i < 5; i++) ins.run(i);

// T7: close() inside a running UDF is deferred to the end of the outer call;
// the outer query reports the closed database, no partial rows, and every
// later call sees it closed.
let ran = 0;
db.function("closer", (x: number) => {
  ran++;
  if (x === 2) {
    db.close();
    console.log("t7 inside isOpen", db.isOpen);
  }
  return x;
});
try {
  console.log("t7 rows", JSON.stringify(db.prepare("select closer(a) as v from t order by a").all()));
} catch (e: any) {
  console.log("t7 outer", e.code, e.message);
}
console.log("t7 ran", ran, "isOpen", db.isOpen);
try {
  db.exec("select 1");
} catch (e: any) {
  console.log("t7 later", e.code, e.message);
}

// open() inside the callback that closed the database is refused; after the
// outer call returns, open() works on the same object.
db.open();
db.exec("create table u(a integer)");
db.exec("insert into u values (1), (2)");
db.function("reopener", (x: number) => {
  db.close();
  try {
    db.open();
  } catch (e: any) {
    console.log("t7 open inside", e.code, e.message);
  }
  return x;
});
try {
  db.prepare("select reopener(a) from u").all();
} catch (e: any) {
  console.log("t7 outer2", e.code, e.message);
}
db.open();
console.log("t7 reopened", db.isOpen, (db.prepare("select 41 + 1 as v").get() as any).v);

// L9: the callback closes and then throws: its exception wins over the
// close, and the database is closed afterwards.
const err = new Error("after close");
db.function("closeThrow", () => {
  db.close();
  throw err;
});
try {
  db.prepare("select closeThrow()").get();
} catch (e) {
  console.log("l9", e === err, db.isOpen);
}

// T5: the first throw wins: no step runs after it and the aggregate's
// result callback does not run for the failed group (node 26.5.1 calls
// result and drops the error).
let steps = 0;
let finals = 0;
db.open();
db.exec("create table s(a integer)");
db.exec("insert into s values (0), (1), (2), (3), (4)");
db.aggregate("bad2", {
  start: 0,
  step: (acc: number, x: number) => {
    steps++;
    if (x >= 2) throw new Error("step " + x);
    return acc + x;
  },
  result: (acc: number) => {
    finals++;
    return acc;
  },
});
try {
  db.prepare("select bad2(a) from s").get();
} catch (e: any) {
  console.log("t5", e.message);
}
console.log("t5 steps", steps, "results", finals);
db.close();

// A nested call after close() inside the callback gets a catchable closed
// error; the outer call still ends closed.
db.open();
db.function("nestedAfterClose", () => {
  db.close();
  try {
    db.exec("select 1");
    return "no";
  } catch (e: any) {
    return e.code;
  }
});
try {
  console.log("nested", JSON.stringify(db.prepare("select nestedAfterClose() as v").get()));
} catch (e: any) {
  console.log("nested outer", e.code, e.message);
}

// T6: an aggregate group still open when the connection is released (here
// by a close() from its own step) finishes without running JS.
db.open();
db.exec("create table g(k integer, v integer)");
db.exec("insert into g values (1, 1), (1, 2), (2, 3), (2, 4)");
let results = 0;
db.aggregate("closingSum", {
  start: 0,
  step: (acc: number, v: number) => {
    if (v === 3) db.close();
    return acc + v;
  },
  result: (acc: number) => {
    results++;
    return acc;
  },
});
try {
  db.prepare("select k, closingSum(v) from g group by k order by k").all();
} catch (e: any) {
  console.log("t6 outer", e.code, e.message);
}
console.log("t6 result calls after release", results, db.isOpen);

// T10: databases whose UDF captures the database, never closed: the
// owner <-> payload cycle is ordinary garbage, so RSS stays flat.
function rssMB(): number {
  return process.memoryUsage().rss / 1048576;
}
let sum = 0;
function churn(n: number): void {
  for (let i = 0; i < n; i++) {
    const d = new DatabaseSync(":memory:");
    d.function("f", (x: number) => x + (d.isOpen ? 1 : 0));
    sum += (d.prepare("select f(1) as v").get() as any).v;
  }
}
churn(20000);
const before = rssMB();
churn(200000);
const grew = rssMB() - before;
console.log("t10", sum, grew < 64 ? "flat" : "grew " + Math.round(grew) + "MB");
