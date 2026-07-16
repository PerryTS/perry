import { Worker } from "node:worker_threads";

process.chdir("test-parity/node-suite/worker_threads/worker-lifecycle");

const data: any = { child: { value: 9 } };
data.self = data;
data.child.parent = data;

try {
  const worker = new Worker("./workerdata-cycle-worker.cjs", {
    workerData: data,
  });
  worker.on(
    "message",
    (message) =>
      console.log("cycle:", message.cycle, message.nested, message.value),
  );
  worker.on(
    "error",
    (error) => console.log("error:", error.name, (error as any).code ?? ""),
  );
  worker.on("exit", (code) => console.log("exit:", code));
} catch (error) {
  console.log("construct:", (error as any)?.name, (error as any)?.code ?? "");
}
