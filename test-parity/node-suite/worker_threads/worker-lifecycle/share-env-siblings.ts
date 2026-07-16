import { SHARE_ENV, Worker } from "node:worker_threads";

process.chdir("test-parity/node-suite/worker_threads/worker-lifecycle");

process.env.PERRY_SHARED_SIBLING = "parent";
process.env.PERRY_SHARED_DELETE = "delete-me";

function run(
  command: "mutate" | "inspect",
): Promise<{ message: any; code: number }> {
  return new Promise((resolve) => {
    const worker = new Worker("./share-env-sibling-worker.cjs", {
      env: SHARE_ENV,
    });
    worker.on("message", async (message: any) => {
      if (message.phase === "ready") {
        worker.postMessage(command);
        return;
      }
      const code = await worker.terminate();
      resolve({ message, code });
    });
  });
}

async function main() {
  const first = await run("mutate");
  console.log("first:", first.message.value, first.message.deleted);

  const second = await run("inspect");
  console.log(
    "second:",
    second.message.value,
    second.message.deleted,
    second.message.enumerated,
  );
  console.log(
    "parent:",
    process.env.PERRY_SHARED_SIBLING,
    process.env.PERRY_SHARED_DELETE === undefined,
  );
  console.log("terminate:", first.code, second.code);
}

main().catch((error) =>
  console.log("unexpected:", error?.name, error?.message)
);
