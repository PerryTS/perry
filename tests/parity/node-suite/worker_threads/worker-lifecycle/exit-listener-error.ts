import { Worker } from "node:worker_threads";

const events: string[] = [];

process.once("uncaughtException", (error: any) => {
  events.push(`uncaught:${error?.message}`);
  console.log("events:", events.join(","));
});

const worker = new Worker("", { eval: true });
worker.on("exit", () => {
  events.push("exit");
  throw new Error("exit-listener-boom");
});
