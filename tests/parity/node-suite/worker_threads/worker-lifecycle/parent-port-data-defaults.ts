import { Worker } from "node:worker_threads";

process.chdir(new URL(".", import.meta.url).pathname);
const worker = new Worker("./fixtures/parent-port-data-defaults-worker.cjs");
const inputs: [string, any][] = [["undefined", undefined], ["null", null], ["number", 7]];
let index = 0;
worker.on("message", (rows) => {
  console.log(inputs[index][0], JSON.stringify(rows));
  if (++index < inputs.length) worker.postMessage(inputs[index][1]);
  else worker.postMessage("stop");
});
worker.on("exit", (code) => { console.log("complete", index, code); });
worker.postMessage(inputs[0][1]);
