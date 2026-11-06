import { parallelFilter, parallelMap, spawn } from "perry/thread";

async function spawnNonBlocking(): Promise<void> {
  console.log("1. Starting background work");

  const bgThread = spawn(() => {
    // Runs on a background thread - heavier work elided here.
    let n = 0;
    for (let i = 0; i < 10; i++) n++;
    return n;
  });

  console.log("2. Main thread continues immediately");

  const result: number = await bgThread;
  console.log(`3. Got result: ${result.toLocaleString("en-US")}`);
}

await spawnNonBlocking();
