// SNAPREC2 F5: a program with a Worker and top-level await. The worker starts from the entry body (after the cut).
// gc() marks the cut: a worker that runs under the census aborts it (a foreign-thread fault on a tracked page),
// so the worker starts after the boundary, as in the real run after the image.
// Node prints: main evals=1 tid=0 / worker evals=101 tid=1 / main after evals=1.
import { Worker, isMainThread, parentPort } from "node:worker_threads";
import { rec } from "./state.ts";
await 0;
if (isMainThread) {
  console.log(`main evals=${rec.evals} tid=${rec.tid}`);
  gc();
  const w = new Worker(new URL(import.meta.url));
  w.on("message", (m: string) => console.log("worker", m));
  w.on("exit", () => console.log(`main after evals=${rec.evals}`));
} else {
  rec.evals += 100;
  parentPort!.postMessage(`evals=${rec.evals} tid=${rec.tid}`);
}
