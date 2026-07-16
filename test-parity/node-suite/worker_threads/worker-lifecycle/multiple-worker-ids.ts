import { Worker } from "node:worker_threads";

process.chdir("test-parity/node-suite/worker_threads/worker-lifecycle");

function observe(
  worker: Worker,
): Promise<{ reported: number | undefined; exit: number }> {
  return new Promise((resolve) => {
    let reported: number | undefined;
    worker.on("message", (message) => reported = message.threadId);
    worker.on("exit", (exit) => resolve({ reported, exit }));
  });
}

async function main() {
  const first = new Worker("./thread-metadata-worker.cjs", { name: "first" });
  const second = new Worker("./thread-metadata-worker.cjs", { name: "second" });
  const parentIds = [first.threadId, second.threadId];
  const observations = await Promise.all([observe(first), observe(second)]);

  console.log("parent positive:", parentIds.every((id) => id > 0));
  console.log("parent unique:", new Set(parentIds).size === 2);
  console.log(
    "reported:",
    observations.map((item) => item.reported ?? "missing").join(","),
  );
  console.log(
    "reported match:",
    observations[0].reported === parentIds[0],
    observations[1].reported === parentIds[1],
  );
  console.log("exits:", observations.map((item) => item.exit).join(","));
  console.log("increasing:", parentIds[1] > parentIds[0]);
}

main().catch((error) =>
  console.log("unexpected:", error?.name, error?.message)
);
