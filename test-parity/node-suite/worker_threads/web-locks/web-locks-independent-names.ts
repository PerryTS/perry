import { locks } from "node:worker_threads";

async function main() {
  const events: string[] = [];
  let releaseA: () => void = () => {};

  const a = locks.request("lock-a", () => {
    events.push("a-held");
    return new Promise<void>((resolve) => {
      releaseA = resolve;
    });
  });

  const b = locks.request("lock-b", () => {
    events.push("b-held");
    return "b-result";
  });

  console.log("b result:", await b);
  console.log("before release:", events.join(","));
  releaseA();
  await a;
  console.log("after release:", events.join(","));
}

main().catch((error) =>
  console.log("unexpected:", error?.name, error?.message)
);
