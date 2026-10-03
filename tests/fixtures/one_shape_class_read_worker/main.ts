import { Worker } from "node:worker_threads";

// Compare this same read site with and without a real Worker start. Once any
// worker exists, class-read entries must decline on both agents.
class NodeObject {
  kind = 1;
}
const nodes: any[] = [];
for (let n = 0; n < 12; n++) {
  const node: any = new NodeObject();
  for (let k = 0; k < n; k++) node["extra" + k] = k;
  nodes.push(node);
}
function read(node: any): number {
  const value = node.optional;
  return value === undefined ? 0 : value;
}
function sumReads(): number {
  let sum = 0;
  for (let i = 0; i < 1200; i++) sum += read(nodes[i % nodes.length]);
  return sum;
}
function collect(): void {
  let churn: any[] = [];
  for (let i = 0; i < 20000; i++) churn.push({ i });
  (globalThis as any).gc();
  churn = [];
}

console.log("before", sumReads());
if (process.env.A2_START_WORKER !== "1") {
  collect();
  console.log("after", sumReads());
  console.log("after-again", sumReads());
} else {
  process.chdir("tests/fixtures/one_shape_class_read_worker");
  const worker = new Worker("./worker.cjs");
  worker.on("message", (message: any) => {
    if (message === "ready") {
      collect();
      console.log("after", sumReads());
      console.log("after-again", sumReads());
      worker.postMessage("go");
    } else {
      console.log("worker", message);
      worker.terminate();
    }
  });
  worker.on("exit", (code: number) => console.log("worker-exit", code));
}
