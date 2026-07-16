import { Worker } from "node:worker_threads";

const values: Array<[string, any]> = [
  ["undefined", undefined],
  ["null", null],
  ["false", false],
  ["zero", 0],
  ["symbol", Symbol("worker")],
  ["object", {}],
  ["array", []],
  ["function", () => {}],
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
