const { SHARE_ENV, Worker } = require("node:worker_threads");

const before = process.env.PERRY_FOUNDER_KEY;
const worker = new Worker("./share-env-founder-b-worker.cjs", {
  env: SHARE_ENV,
});

worker.on("error", () => {
  process.exitCode = 1;
});
worker.on("exit", code => {
  if (
    code !== 0 || before !== "from-A" ||
    process.env.PERRY_FOUNDER_KEY !== "from-A"
  ) {
    process.exitCode = 1;
  }
});
