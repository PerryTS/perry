import { Worker } from "node:worker_threads";

const values: Array<[string, any]> = [
  ["bare relative", "worker.cjs"],
  ["http string", "https://example.com/worker.js"],
  ["http URL", new URL("https://example.com/worker.js")],
];

async function main() {
  for (const [label, value] of values) {
    let worker: Worker | undefined;
    try {
      worker = new Worker(value);
      console.log(label, "created");
    } catch (error: any) {
      console.log(label, error?.name, error?.code ?? "");
    } finally {
      await worker?.terminate().catch(() => {});
    }
  }
}

main().catch((error) =>
  console.log("unexpected:", error?.name, error?.message)
);
