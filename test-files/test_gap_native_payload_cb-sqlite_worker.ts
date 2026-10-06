// #11919 T11: a worker opens a node:sqlite database with a UDF that captures
// it and an aggregate, queries it and ends without closing it. Worker
// teardown releases the open database (no JS runs during the release) and the
// main thread carries on with its own database.
import { Worker } from "node:worker_threads";
import { DatabaseSync } from "node:sqlite";

const worker = new Worker(
  new URL("./_helpers/gap_cb_sqlite_worker.ts", import.meta.url),
);
const watchdog = setTimeout(() => {
  console.log("WORKER STOPPED");
  process.exit(3);
}, 15000);
const first = await new Promise<string>((resolve) => {
  worker.on("message", (value: string) => resolve(String(value)));
  worker.on("error", (e: Error) => resolve("worker-error " + e.message));
});
console.log(first);
await worker.terminate();
clearTimeout(watchdog);
console.log("terminated");

const db = new DatabaseSync(":memory:");
db.function("g", (x: number) => x * 3);
console.log("main", (db.prepare("select g(14) as v").get() as any).v);
db.close();
