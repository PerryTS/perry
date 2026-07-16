import { setEnvironmentData, Worker } from "node:worker_threads";

process.chdir("test-parity/node-suite/worker_threads/environment-data");

const shared = { value: 11 };
const graph: any = { left: shared, right: shared };
graph.self = graph;
setEnvironmentData("graph-snapshot", graph);

try {
  const worker = new Worker("./graph-snapshot-worker.cjs");
  worker.on(
    "message",
    (message) =>
      console.log("graph:", message.cycle, message.alias, message.value),
  );
  worker.on(
    "error",
    (error) => console.log("error:", error.name, (error as any).code ?? ""),
  );
  worker.on("exit", (code) => {
    console.log("exit:", code);
    setEnvironmentData("graph-snapshot", undefined);
  });
} catch (error) {
  console.log("construct:", (error as any)?.name, (error as any)?.code ?? "");
  setEnvironmentData("graph-snapshot", undefined);
}
