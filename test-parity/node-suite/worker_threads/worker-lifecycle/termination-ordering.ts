import { Worker } from "node:worker_threads";

process.chdir("test-parity/node-suite/worker_threads/worker-lifecycle");

const events: string[] = [];
let exitCode: number | undefined;
let terminateCode: number | undefined;

function finish() {
  if (exitCode === undefined || terminateCode === undefined) return;
  console.log("lifecycle:", events.join(","));
  console.log("codes:", exitCode, terminateCode);
}

const worker = new Worker("./termination-worker.cjs");
worker.on("online", () => events.push("online"));
worker.on("message", (message) => {
  events.push(`message:${message}`);
  worker.terminate().then((code) => {
    events.push("terminate-resolved");
    terminateCode = code;
    finish();
  });
});
worker.on("exit", (code) => {
  events.push("exit");
  exitCode = code;
  finish();
});
