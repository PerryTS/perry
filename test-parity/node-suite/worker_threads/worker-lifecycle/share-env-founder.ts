import { SHARE_ENV, Worker } from "node:worker_threads";

process.chdir("test-parity/node-suite/worker_threads/worker-lifecycle");

function wait(worker: Worker): Promise<number> {
  return new Promise((resolve) => {
    worker.on("error", () => {});
    worker.on("exit", resolve);
  });
}

async function main() {
  const previous = process.env.PERRY_FOUNDER_KEY;
  process.env.PERRY_FOUNDER_KEY = "from-main";

  try {
    await wait(
      new Worker("./constructor-validation-worker.cjs", { env: SHARE_ENV }),
    );
    const code = await wait(
      new Worker("./share-env-founder-a-worker.cjs", {
        env: { PERRY_FOUNDER_KEY: "from-A" },
      }),
    );
    console.log("founder:", code, process.env.PERRY_FOUNDER_KEY);
  } finally {
    if (previous === undefined) delete process.env.PERRY_FOUNDER_KEY;
    else process.env.PERRY_FOUNDER_KEY = previous;
  }
}

main().catch((error) =>
  console.log("unexpected:", error?.name, error?.message)
);
