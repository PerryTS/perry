import { locks } from "node:worker_threads";

function summarize(snapshot: any) {
  const held = snapshot.held.map((item: any) => `${item.name}:${item.mode}:${typeof item.clientId}`);
  const pending = snapshot.pending.map((item: any) => `${item.name}:${item.mode}:${typeof item.clientId}`);
  return `${held.join("|")} / ${pending.join("|")}`;
}

async function main() {
  let releaseExclusive: (value: string) => void = () => {};

  const first = locks.request("query-lock", (lock: any) => {
    console.log("first lock:", lock.name, lock.mode);
    return new Promise((resolve) => {
      releaseExclusive = resolve as (value: string) => void;
    });
  });

  const second = locks.request("query-lock", { mode: "shared" }, (lock: any) => {
    console.log("second lock:", lock.name, lock.mode);
    return "second-result";
  });

  const pendingSnapshot = await locks.query();
  console.log("pending snapshot:", summarize(pendingSnapshot));

  releaseExclusive("first-result");
  console.log("first result:", await first);
  console.log("second result:", await second);

  const finalSnapshot = await locks.query();
  console.log("final snapshot:", summarize(finalSnapshot));
}

main().catch((err) => {
  console.log("error:", err?.name, err?.code, err?.message);
});
