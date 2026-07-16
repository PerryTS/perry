import { SHARE_ENV, Worker } from "node:worker_threads";

process.chdir("test-parity/node-suite/worker_threads/worker-lifecycle");
process.env[123] = "from-main";
process.env[7] = "delete-me";

const worker = new Worker("./share-env-indexed-worker.cjs", { env: SHARE_ENV });
worker.on("message", (message: any) => {
  console.log("worker:", message.initial, message.keys.join(","));
});
worker.on("exit", (code) => {
  console.log("main:", process.env[123], process.env[456], process.env[7]);
  console.log("exit:", code);
});
