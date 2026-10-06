// Worker half of test_gap_native_payload_cb-sqlite_worker.ts: a database
// with a UDF that captures it and an aggregate, queried once. The database is
// never closed: worker teardown releases it. (Perry cannot yet preempt a
// worker busy in a loop, so the worker ends on its own rather than being
// terminated mid-query.)
import { parentPort } from "node:worker_threads";
import { DatabaseSync } from "node:sqlite";

const db = new DatabaseSync(":memory:");
db.exec("create table t(a integer)");
for (let i = 0; i < 100; i++) db.exec("insert into t values (" + i + ")");
db.function("f", (x: number) => x + (db.isOpen ? 1 : 0));
db.aggregate("s", { start: 0, step: (acc: number, x: number) => acc + x, result: (v: number) => v });
const stmt = db.prepare("select s(f(a)) as v from t");
parentPort!.postMessage("worker " + (stmt.get() as any).v);
