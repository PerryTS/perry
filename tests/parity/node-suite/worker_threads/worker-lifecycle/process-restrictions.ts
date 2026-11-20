import { isMainThread, Worker } from "node:worker_threads";

console.log("main:", isMainThread, (process.abort as any).disabled === true);

function run(env: Record<string, string>): Promise<void> {
  return new Promise((resolve) => {
    const worker = new Worker(new URL("./process-restrictions-worker.cjs", import.meta.url), { env });
    worker.on("message", (message) => console.log("worker:", JSON.stringify(message)));
    worker.on("error", (error: any) => console.log("error:", error.name, error.code));
    worker.on("exit", (code) => { console.log("exit:", code); resolve(); });
  });
}

async function main() {
  await run({});
  await run({ NODE_CHANNEL_FD: "worker-parity" });
}
main().catch((error) => console.log("unexpected:", error.name, error.code));
