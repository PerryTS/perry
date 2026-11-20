import { Worker } from "node:worker_threads";

function utilizationShape(label: string, value: any) {
  console.log(
    label,
    Object.keys(value).sort().join(","),
    typeof value.active,
    typeof value.idle,
    typeof value.utilization,
  );
}

const worker = new Worker("setInterval(() => {}, 1000);", { eval: true });
worker.once("online", async () => {
  const cpu = await worker.cpuUsage();
  console.log("cpu:", Object.keys(cpu).sort().join(","), typeof cpu.user, typeof cpu.system);
  utilizationShape("live:", worker.performance.eventLoopUtilization());
  await worker.terminate();
  utilizationShape("exited:", worker.performance.eventLoopUtilization());
});
