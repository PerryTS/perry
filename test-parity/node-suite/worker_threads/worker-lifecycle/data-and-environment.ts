import {
  setEnvironmentData,
  Worker,
} from "node:worker_threads";

process.chdir("test-parity/node-suite/worker_threads/worker-lifecycle");

const environment = { label: "snapshot", version: 1 };
setEnvironmentData("suite-environment", environment);

const buffer = new ArrayBuffer(4);
const view = new Uint8Array(buffer);
view.set([1, 2, 3, 4]);
const events: string[] = [];

const worker = new Worker("./data-worker.cjs", {
  name: "data-worker",
  workerData: { buffer, view, nested: { value: "worker-value" } },
  transferList: [buffer],
});

environment.version = 2;
console.log("parent detached:", buffer.byteLength, view.byteLength);

worker.on("online", () => events.push("online"));
worker.on("message", (message: any) => {
  events.push("message");
  console.log(
    "worker data:",
    message?.isMainThread,
    message?.threadName,
    message?.buffer,
    message?.view,
    message?.sharedBacking,
    message?.values,
    message?.nested,
  );
  console.log("environment:", message?.environment);
});
worker.on("exit", (code) => {
  events.push("exit");
  console.log("lifecycle:", events.join(","), code);
  setEnvironmentData("suite-environment", undefined);
});
