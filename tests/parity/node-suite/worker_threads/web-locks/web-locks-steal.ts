import { locks } from "node:worker_threads";

async function main() {
  let release: () => void = () => {};
  const events: string[] = [];
  const held = locks.request("steal-lock", () => {
    events.push("held");
    return new Promise<void>((resolve) => {
      release = resolve;
    });
  });

  const stolen = locks.request("steal-lock", { steal: true }, () => {
    events.push("stolen");
    return "stolen-result";
  });

  try {
    await held;
    events.push("held-resolved");
  } catch (error: any) {
    events.push(`held-${error?.name}`);
  }

  console.log("stolen result:", await stolen);
  release();
  console.log("events:", events.join(","));
  const snapshot = await locks.query();
  console.log("final:", snapshot.held.length, snapshot.pending.length);
}

main().catch((error) => console.log("unexpected:", error?.name, error?.message));
