const { SHARE_ENV, Worker } = require("node:worker_threads");

process.env.PERRY_TREE_A = "a";
const worker = new Worker("./share-env-tree-b-worker.cjs", { env: SHARE_ENV });
worker.on("error", () => {
  process.exitCode = 1;
});
worker.on("exit", code => {
  if (code !== 0) process.exitCode = 1;
});
