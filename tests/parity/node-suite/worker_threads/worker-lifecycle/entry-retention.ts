import { Worker } from "node:worker_threads";

process.chdir("tests/parity/node-suite/worker_threads/worker-lifecycle");

async function main() {
  const evalMessage = await new Promise((resolve) => {
    const worker = new Worker(
      'require("node:worker_threads").parentPort.postMessage("eval entry");',
      { eval: true },
    );
    worker.on("message", resolve);
  });
  console.log("eval:", evalMessage);
  const fileMessage = await new Promise((resolve) => {
    const worker = new Worker("./entry-retention-worker.cjs");
    worker.on("message", resolve);
  });
  console.log("file dynamic import:", fileMessage);
}

main().catch((error) => console.log("unexpected:", error.name, error.message));
