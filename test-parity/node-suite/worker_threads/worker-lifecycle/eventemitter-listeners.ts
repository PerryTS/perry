import { Worker } from "node:worker_threads";

process.chdir("test-parity/node-suite/worker_threads/worker-lifecycle");

const worker = new Worker("./eventemitter-worker.cjs");
const events: string[] = [];

function removed() {
  events.push("removed");
}

worker.on("message", removed);
worker.off("message", removed);
worker.once("online", () => events.push("online-once"));
worker.on("online", () => events.push("online-on"));
worker.once("message", (value) => events.push(`once:${value}`));
worker.on("message", (value) => events.push(`on:${value}`));
worker.on("exit", (code) => {
  events.push(`exit:${code}`);
  console.log("events:", events.join(","));
  const count = typeof (worker as any).listenerCount === "function"
    ? `${(worker as any).listenerCount("message")},${
      (worker as any).listenerCount("online")
    }`
    : "unsupported";
  console.log("counts:", count);
});
