import { Worker } from "node:worker_threads";

const worker = new Worker(
  `
    Error.prepareStackTrace = (error) => {
      if (error.message === "stack-boom") throw new Error("prepare-boom");
      return "custom-stack";
    };
    throw new Error("stack-boom");
  `,
  { eval: true },
);

let errorSummary = "missing";
worker.on("error", (error: any) => {
  errorSummary = `${error?.name}:${error?.message}:${
    error?.stack === undefined
  }`;
});
worker.on("exit", (code) => {
  console.log("error:", errorSummary);
  console.log("exit:", code);
});
