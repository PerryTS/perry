import { Worker, isMainThread } from "node:worker_threads";
import { record, settled } from "./a.ts";
import { later } from "./b.ts";
gc();
if (isMainThread) {
  const w = new Worker(new URL(import.meta.url));
  w.on("exit", () => console.log("main", record.evals, settled, later));
} else {
  record.evals += 100;
}
