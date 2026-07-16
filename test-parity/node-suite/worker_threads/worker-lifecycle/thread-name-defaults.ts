import { Worker } from "node:worker_threads";

process.chdir("test-parity/node-suite/worker_threads/worker-lifecycle");

const cases: Array<[string, boolean, any]> = [
  ["missing", false, undefined],
  ["empty", true, ""],
  ["false", true, false],
  ["zero", true, 0],
  ["null", true, null],
  ["nan", true, Number.NaN],
];

async function run(label: string, includeName: boolean, name: any) {
  const options = includeName ? { name } : {};
  const worker = new Worker("./thread-metadata-worker.cjs", options);

  const message = await new Promise<any>((resolve, reject) => {
    worker.once("message", resolve);
    worker.once("error", reject);
  });
  console.log(label, worker.threadName, message.threadName);
  await new Promise<void>((resolve) => worker.once("exit", () => resolve()));
}

async function main() {
  for (const entry of cases) await run(...entry);
}

main().catch((error) =>
  console.log("unexpected:", error?.name, error?.code ?? "")
);
