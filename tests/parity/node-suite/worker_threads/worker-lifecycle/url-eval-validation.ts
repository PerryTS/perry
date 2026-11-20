import { Worker } from "node:worker_threads";

const filename = new URL(
  "./internal-thread-worker.cjs",
  import.meta.url,
);

try {
  const worker = new Worker(filename, { eval: true });
  console.log("construct: ok");
  worker.on("error", (error: any) => {
    console.log("error:", error?.name, error?.code ?? "");
  });
  worker.on("exit", (code) => console.log("exit:", code));
  worker.terminate();
} catch (error: any) {
  console.log("construct:", error?.name, error?.code ?? "");
}
