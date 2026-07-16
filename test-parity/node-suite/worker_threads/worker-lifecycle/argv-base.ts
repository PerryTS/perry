import { Worker } from "node:worker_threads";

process.chdir("test-parity/node-suite/worker_threads/worker-lifecycle");

async function run(label: string, worker: Worker) {
  let message: any;
  let error: any;
  worker.once("message", (value) => {
    message = value;
  });
  worker.once("error", (value) => {
    error = value;
  });
  const code = await new Promise<number>((resolve) =>
    worker.once("exit", resolve)
  );
  console.log(
    label,
    message?.exec,
    message?.script,
    JSON.stringify(message?.tail),
    error?.name ?? "no-error",
    code,
  );
}

async function main() {
  await run(
    "file:",
    new Worker("./argv-base-worker.cjs", { argv: [null, 42] as any }),
  );
  await run(
    "eval:",
    new Worker(
      `
      const { parentPort } = require("node:worker_threads");
      parentPort.postMessage({
        exec: process.argv[0] === process.execPath,
        script: process.argv[1] === "[worker eval]",
        tail: process.argv.slice(2),
      });
      `,
      { eval: true, argv: [null, 42] as any },
    ),
  );
}

main().catch((error) =>
  console.log("unexpected:", error?.name, error?.message)
);
