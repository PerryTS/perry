import { SHARE_ENV, Worker } from "node:worker_threads";

process.chdir("test-parity/node-suite/worker_threads/worker-lifecycle");

function wait(worker: Worker): Promise<void> {
  return new Promise((resolve, reject) => {
    worker.on("error", reject);
    worker.on("exit", (code) => {
      if (code === 0) resolve();
      else reject(new Error("exit:" + code));
    });
  });
}

async function main() {
  const previousMain = process.env.PERRY_TREE_MAIN;
  const previousA = process.env.PERRY_TREE_A;
  const previousB = process.env.PERRY_TREE_B;
  const previousC = process.env.PERRY_TREE_C;
  const previousCSeesB = process.env.PERRY_TREE_C_SEES_B;
  const previousCSeesMain = process.env.PERRY_TREE_C_SEES_MAIN;
  process.env.PERRY_TREE_MAIN = "main";
  delete process.env.PERRY_TREE_A;
  delete process.env.PERRY_TREE_B;
  delete process.env.PERRY_TREE_C;
  delete process.env.PERRY_TREE_C_SEES_B;
  delete process.env.PERRY_TREE_C_SEES_MAIN;
  try {
    await wait(new Worker("./share-env-tree-a-worker.cjs"));
    await wait(
      new Worker("./share-env-tree-c-worker.cjs", { env: SHARE_ENV }),
    );
    console.log(
      "C observations:",
      process.env.PERRY_TREE_C_SEES_B === "missing"
        ? null
        : process.env.PERRY_TREE_C_SEES_B ?? null,
      process.env.PERRY_TREE_C_SEES_MAIN ?? null,
    );
    console.log(
      "main:",
      process.env.PERRY_TREE_B ?? null,
      process.env.PERRY_TREE_C ?? null,
    );
  } finally {
    for (
      const [key, value] of [
        ["PERRY_TREE_MAIN", previousMain],
        ["PERRY_TREE_A", previousA],
        ["PERRY_TREE_B", previousB],
        ["PERRY_TREE_C", previousC],
        ["PERRY_TREE_C_SEES_B", previousCSeesB],
        ["PERRY_TREE_C_SEES_MAIN", previousCSeesMain],
      ] as const
    ) {
      if (value === undefined) delete process.env[key];
      else process.env[key] = value;
    }
  }
}

main().catch((error) =>
  console.log("unexpected:", error?.name, error?.message)
);
