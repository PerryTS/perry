import { Worker } from "node:worker_threads";

const worker = new Worker(
  `
    throw new TypeError("eval-boom");
  `,
  { eval: true },
);

const events: string[] = [];
worker.on("online", () => events.push("online"));
worker.on("error", (error: any) => {
  events.push(`error:${error?.name}:${error?.message}`);
});
worker.on("exit", (code) => {
  events.push(`exit:${code}`);
  console.log("events:", events.join(","));
});
