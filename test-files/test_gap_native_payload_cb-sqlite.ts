// #11919: node:sqlite DatabaseSync / StatementSync / StatementSyncIterator /
// SQLTagStore / Session are ordinary objects that own a native payload, and
// SQLite calls back into JS (UDFs, aggregates, authorizer) through the
// database's traced owner edge. Shape, identity, instanceof, close/reopen
// errors, callbacks, throws from callbacks, reentrancy and churn, against node.
import { DatabaseSync, StatementSync, Session, constants } from "node:sqlite";

function shape(label: string, o: any): void {
  console.log(
    label,
    typeof o,
    JSON.stringify(Object.keys(o)),
    Object.prototype.toString.call(o),
    o.constructor.name,
  );
}
function protoNames(o: any): string {
  return Object.getOwnPropertyNames(Object.getPrototypeOf(o)).sort().join(",");
}
function attempt(label: string, f: () => unknown): void {
  try {
    console.log(label, "ok", JSON.stringify(f()));
  } catch (e: any) {
    console.log(label, "threw", e.name, e.code, e.message);
  }
}

const db = new DatabaseSync(":memory:");
const db2 = new DatabaseSync(":memory:");
shape("db", db);
console.log("db json", JSON.stringify(db));
console.log("db proto", protoNames(db));
console.log("identity", db === db2, db !== db2, db === db);
console.log(
  "instanceof",
  db instanceof DatabaseSync,
  db2 instanceof DatabaseSync,
  ({}) instanceof DatabaseSync,
  db.constructor === DatabaseSync,
);

db.exec("create table t(a integer, b text)");
const ins = db.prepare("insert into t values(?, ?)");
shape("stmt", ins);
console.log("stmt json", JSON.stringify(ins));
console.log("stmt proto", protoNames(ins));
console.log("stmt instanceof", ins instanceof StatementSync, ins.constructor === StatementSync);
for (let i = 0; i < 5; i++) ins.run(i, "r" + i);
console.log("expanded", ins.expandedSQL);

const sel = db.prepare("select a, b from t order by a");
const it = sel.iterate();
shape("iter", it);
console.log("iter proto", protoNames(it));
console.log("iter", JSON.stringify(it.next()), [...it].length, JSON.stringify(it.next()));
const it2 = sel.iterate();
sel.all();
attempt("iter invalidated", () => it2.next());
const it3 = sel.iterate();
console.log("iter return", JSON.stringify(it3.return!()), JSON.stringify(it3.next()));

const ts = db.createTagStore(2);
shape("tags", ts);
console.log("tags db", ts.db === db, ts.size, ts.capacity);
console.log("tags get", JSON.stringify(ts.get`select count(*) as n from t where a < ${3}`));
console.log("tags all", JSON.stringify(ts.all`select a from t where a > ${2} order by a`));
ts.run`insert into t values(${10}, ${"ten"})`;
console.log("tags size", ts.size);
ts.clear();
console.log("tags cleared", ts.size);

const s = db.createSession();
shape("session", s);
console.log("session proto", protoNames(s), s instanceof Session);
db.exec("insert into t values(20, 'twenty')");
const changes = s.changeset();
console.log("changeset", changes instanceof Uint8Array, changes.length > 0);
s.close();
attempt("session closed", () => s.changeset());

const m = new Map<any, number>([[db, 1], [db2, 2]]);
const w = new WeakMap<any, string>([[ins, "ins"], [ts, "ts"]]);
console.log("keys", m.get(db), m.get(db2), m.size, w.get(ins), w.get(ts));

// Callbacks: scalar, aggregate, window, authorizer.
db.function("twice", (x: number) => x * 2);
console.log("udf", db.prepare("select twice(21) as v").get()!.v);
db.function("nested", (x: number) => (db.prepare("select twice(?) as v").get(x) as any).v + 1);
console.log("reentrant udf", db.prepare("select nested(5) as v").get()!.v);
db.function("writer", (x: number) => {
  db.exec("insert into t values(100, 'w')");
  return x;
});
// A callback allocating ~10 MB per call moves the young heap mid-query.
db.function("big", (x: number) => {
  let n = 0;
  for (let i = 0; i < 20; i++) n += new Array(1 << 16).fill(x).length;
  return n + x;
});
console.log("allocating udf", JSON.stringify(db.prepare("select big(a) as v from t order by a").all()));
console.log(
  "udf writes",
  db.prepare("select writer(1) as v").get()!.v,
  db.prepare("select count(*) as n from t where a = 100").get()!.n,
);
db.aggregate("sumsq", {
  start: () => 0,
  step: (acc: number, x: number) => acc + x * x,
  result: (v: number) => "r" + v,
});
console.log(
  "aggregate",
  db.prepare("select sumsq(a) as v from t where a < 5").get()!.v,
  db.prepare("select sumsq(a) as v from t where a > 1000").get()!.v,
);
db.aggregate("win", {
  start: 0,
  step: (a: number, x: number) => a + x,
  inverse: (a: number, x: number) => a - x,
  result: (a: number) => a * 10,
});
console.log(
  "window",
  JSON.stringify(
    db
      .prepare(
        "select a, win(a) over (order by a rows between 1 preceding and current row) as w from t where a < 5 order by a",
      )
      .all(),
  ),
);
let authCalls = 0;
db.setAuthorizer((action: number) => {
  authCalls++;
  return action === constants.SQLITE_DELETE ? constants.SQLITE_DENY : constants.SQLITE_OK;
});
attempt("authorizer deny", () => db.prepare("delete from t"));
console.log("authorizer ran", authCalls > 0);
let inAuth = false;
db.setAuthorizer(() => {
  if (!inAuth) {
    inAuth = true;
    db.exec("select 1");
    inAuth = false;
  }
  return constants.SQLITE_OK;
});
console.log("authorizer reentry", db.prepare("select 7 as v").get()!.v);
db.setAuthorizer(null);

// Throws from callbacks surface as the exact value, outside SQLite.
class MyErr extends Error {}
const boom = new MyErr("boom");
db.function("boom", () => {
  throw boom;
});
try {
  db.prepare("select boom()").get();
} catch (e) {
  console.log("udf throw", e === boom, e instanceof MyErr);
}
console.log("reusable", db.prepare("select twice(2) as v").get()!.v);
let steps = 0;
db.aggregate("bad", {
  start: 0,
  step: (_acc: number, _x: number) => {
    steps++;
    throw new Error("step" + steps);
  },
});
attempt("aggregate throw", () => db.prepare("select bad(a) from t").get());
console.log("first throw wins", steps);
db.setAuthorizer(() => "x" as any);
attempt("authorizer non-int", () => db.prepare("select 1"));
db.setAuthorizer(() => 99);
attempt("authorizer bad code", () => db.prepare("select 1"));
const authThrow = new Error("auth boom");
db.setAuthorizer(() => {
  throw authThrow;
});
try {
  db.prepare("select 1");
} catch (e) {
  console.log("authorizer throw", e === authThrow);
}
db.setAuthorizer(null);
db.function("tryNested", () => {
  try {
    db.exec("select boom()");
  } catch (e) {
    return e === boom ? "caught" : "other";
  }
  return "none";
});
console.log("nested throw caught", db.prepare("select tryNested() as v").get()!.v);

// Close, reopen, close-then-use.
const st = db.prepare("select twice(a) as v from t order by a");
const live = st.iterate();
db.close();
console.log("closed", db.isOpen);
attempt("stmt after close", () => st.all());
attempt("iter after close", () => live.next());
attempt("exec after close", () => db.exec("select 1"));
attempt("prepare after close", () => db.prepare("select 1"));
attempt("tags after close", () => ts.get`select 1`);
attempt("double close", () => db.close());
db.open();
console.log("reopened", db.isOpen, db === m.keys().next().value);
attempt("stmt after reopen", () => st.all());
attempt("udf after reopen", () => db.prepare("select twice(1)"));
attempt("double open", () => db.open());
db.close();
db2.close();

// Churn: open, register a UDF capturing the database, query, close.
function rssMB(): number {
  return process.memoryUsage().rss / 1048576;
}
let sum = 0;
for (let i = 0; i < 2000; i++) {
  const d = new DatabaseSync(":memory:");
  d.function("f", (x: number) => x + (d.isOpen ? 1 : 0));
  sum += d.prepare("select f(1) as v").get()!.v as number;
  d.close();
}
const before = rssMB();
for (let i = 0; i < 50000; i++) {
  const d = new DatabaseSync(":memory:");
  d.function("f", (x: number) => x + (d.isOpen ? 1 : 0));
  sum += d.prepare("select f(1) as v").get()!.v as number;
  d.close();
}
const grew = rssMB() - before;
console.log("churn", sum, grew < 64 ? "flat" : "grew " + Math.round(grew) + "MB");
