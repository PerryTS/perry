// Throws on perry/thread workers: each agent walks ITS OWN stack, and the
// walker's row cache + image index are process-global (shared under a
// mutex). Correctness question: does a throw on a worker thread find the
// worker's handler, not the main thread's — and does concurrent access to
// the shared cache stay sound?
import { parallelMap, spawn } from "perry/thread";

// Every element throws and is caught inside the worker.
const caught = parallelMap([1, 2, 3, 4, 5, 6, 7, 8], (n: number): number => {
  let acc = 0;
  for (let i = 0; i < 50; i++) {
    try {
      if ((i & 1) === 0) throw new Error("w" + n + "-" + i);
      acc += 1;
    } catch (e) {
      acc += (e as Error).message.length;
    }
  }
  return acc;
});
console.log("parallelMap:", JSON.stringify(caught));

// Deep unwind inside a worker: the walk must step many worker frames.
function deep(n: number): number {
  if (n === 0) throw new Error("bottom");
  return deep(n - 1) + 1;
}

async function main(): Promise<void> {
  const r = await spawn((): string => {
    let hits = 0;
    for (let k = 0; k < 20; k++) {
      try {
        deep(60);
      } catch {
        hits++;
      }
    }
    return "spawn-caught:" + hits;
  });
  console.log(r);
  console.log("done");
}
main();
