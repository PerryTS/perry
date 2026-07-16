import { Worker } from "node:worker_threads";

const values: Array<[string, any]> = [
  ["undefined", undefined],
  ["null", null],
  ["number", 1],
  ["object", {}],
  ["bare relative", "worker.cjs"],
  ["http string", "https://example.com/worker.js"],
  ["http URL", new URL("https://example.com/worker.js")],
];

for (const [label, value] of values) {
  try {
    new Worker(value);
    console.log(label, "ok");
  } catch (error: any) {
    console.log(label, error?.name, error?.code ?? "");
  }
}
