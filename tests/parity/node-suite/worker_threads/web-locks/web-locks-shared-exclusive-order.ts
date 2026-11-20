import { locks } from "node:worker_threads";

function summarize(snapshot: any) {
  const held = snapshot.held.map((item: any) => `${item.name}:${item.mode}`);
  const pending = snapshot.pending.map((item: any) => `${item.name}:${item.mode}`);
  return `${held.join("|")} / ${pending.join("|")}`;
}

async function main() {
  let releaseA: (value: string) => void = () => {};
  let releaseB: (value: string) => void = () => {};
  const events: string[] = [];

  const sharedA = locks.request("shared-lock", { mode: "shared" }, (lock: any) => {
    events.push(`shared-a:${lock.mode}`);
    return new Promise((resolve) => {
      releaseA = resolve as (value: string) => void;
    });
  });

  const sharedB = locks.request("shared-lock", { mode: "shared" }, (lock: any) => {
    events.push(`shared-b:${lock.mode}`);
    return new Promise((resolve) => {
      releaseB = resolve as (value: string) => void;
    });
  });

  const exclusive = locks.request("shared-lock", { mode: "exclusive" }, (lock: any) => {
    events.push(`exclusive:${lock.mode}`);
    return "exclusive-result";
  });

  console.log("events before release:", events.join(","));
  console.log("snapshot before release:", summarize(await locks.query()));

  releaseA("shared-a-result");
  console.log("after release a:", events.join(","));
  console.log("shared a result:", await sharedA);
  console.log("snapshot after a:", summarize(await locks.query()));

  releaseB("shared-b-result");
  console.log("shared b result:", await sharedB);
  console.log("exclusive result:", await exclusive);
  console.log("events final:", events.join(","));
  console.log("snapshot final:", summarize(await locks.query()));
}

main().catch((err) => {
  console.log("error:", err?.name, err?.code, err?.message);
});
