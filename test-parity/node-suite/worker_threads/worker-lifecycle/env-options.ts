import { Worker } from "node:worker_threads";

process.chdir("test-parity/node-suite/worker_threads/worker-lifecycle");

process.env.PERRY_PARENT_BEFORE = "before";
delete process.env.PERRY_PARENT_AFTER;

const inherited = new Worker("./env-worker.cjs", { workerData: "inherited" });
process.env.PERRY_PARENT_AFTER = "after";

const explicit = new Worker("./env-worker.cjs", {
  workerData: "explicit",
  env: { PERRY_MANUAL: 42 as any, PERRY_BOOLEAN: true as any },
});

for (const worker of [inherited, explicit]) {
  worker.on(
    "message",
    (message) => console.log("env:", JSON.stringify(message)),
  );
  worker.on("exit", (code) => console.log("exit:", code));
}

try {
  new Worker("./env-worker.cjs", { env: 42 as any });
  console.log("invalid env: ok");
} catch (error: any) {
  console.log("invalid env:", error?.name, error?.code ?? "");
}
