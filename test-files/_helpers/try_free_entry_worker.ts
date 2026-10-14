// Worker body for test_gap_try_free_entry_worker.ts.
import { parentPort, workerData } from "node:worker_threads";

function deep(n: number): number {
  if (n === 0) throw new Error("worker deep");
  return deep(n - 1) + 1;
}

const role = workerData as string;
if (role === "throw") {
  deep(100);
} else if (role === "timer") {
  setTimeout(() => {
    throw new Error("worker timer");
  }, 0);
} else if (role === "caught") {
  let caught = 0;
  for (let i = 0; i < 5000; i++) {
    try {
      if (i % 2 === 0) deep(20);
      JSON.parse(i % 3 === 0 ? "{bad" : "1");
    } catch {
      caught++;
    }
  }
  parentPort!.postMessage("caught " + caught);
}
