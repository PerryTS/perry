import { locks } from "node:worker_threads";
import * as workerThreads from "node:worker_threads";

async function main() {
  console.log("locks stable:", locks === workerThreads.locks);
  console.log("request surface:", typeof locks.request, locks.request.name, locks.request.length);
  console.log("query surface:", typeof locks.query, locks.query.name, locks.query.length);

  const result = await locks.request("surface-lock", (lock: any) => {
    console.log("lock fields:", lock.name, lock.mode);
    console.log("lock tostring:", Object.prototype.toString.call(lock));
    return "callback-result";
  });

  console.log("request result:", result);

  const snapshot = await locks.query();
  console.log("empty snapshot:", snapshot.held.length, snapshot.pending.length);
}

main().catch((err) => {
  console.log("error:", err?.name, err?.code, err?.message);
});
