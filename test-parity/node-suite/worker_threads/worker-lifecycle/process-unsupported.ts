import { Worker } from "node:worker_threads";

process.chdir("test-parity/node-suite/worker_threads/worker-lifecycle");

const previousChannel = process.env.NODE_CHANNEL_FD;
process.env.NODE_CHANNEL_FD = "worker-parity";

function restore() {
  if (previousChannel === undefined) {
    delete process.env.NODE_CHANNEL_FD;
  } else {
    process.env.NODE_CHANNEL_FD = previousChannel;
  }
}

try {
  const worker = new Worker("./process-unsupported-worker.cjs");
  worker.on("message", (message) => {
    console.log("writable:", message.title, message.debugPort);
    console.log("stubs:", JSON.stringify(message.stubs));
    console.log("getters:", JSON.stringify(message.getters));
    console.log("umask:", message.umask);
    console.log("internals:", message.internals);
  });
  worker.on(
    "error",
    (error: any) => console.log("error:", error?.name, error?.code ?? ""),
  );
  worker.on("exit", (code) => {
    console.log("exit:", code);
    restore();
  });
} catch (error: any) {
  console.log("construction:", error?.name, error?.code ?? "");
  restore();
}
