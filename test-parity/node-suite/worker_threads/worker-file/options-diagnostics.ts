import { Worker } from "node:worker_threads";

process.chdir("test-parity/node-suite/worker_threads/worker-file");

const limitKeys = [
  "maxYoungGenerationSizeMb",
  "maxOldGenerationSizeMb",
  "codeRangeSizeMb",
  "stackSizeMb",
];

const worker = new Worker("./options-diagnostics-worker.cjs", {
  workerData: { label: "perry", count: 7 },
  name: "surface-worker",
  resourceLimits: {
    maxYoungGenerationSizeMb: 16,
    maxOldGenerationSizeMb: 64,
    codeRangeSizeMb: 8,
    stackSizeMb: 2,
  },
  stdin: true,
  stdout: true,
  stderr: true,
  env: { PERRY_WORKER_ENV_TEST: "worker-env" },
  trackUnmanagedFds: false,
});

function limitsSummary(limits: Record<string, number>) {
  return limitKeys.map((key) => `${key}:${limits[key]}`).join(",");
}

function streamShape(stream: any, kind: "readable" | "writable") {
  if (stream === null) return "null";
  if (kind === "writable") {
    return [
      typeof stream.write,
      stream.writable,
      typeof stream.on,
      stream.write("ignored by fixture"),
    ].join(",");
  }
  return [typeof stream.on, typeof stream.read, stream.readable].join(",");
}

console.log("threadName:", worker.threadName);
console.log("limits:", limitsSummary(worker.resourceLimits));
console.log("stdin shape:", streamShape(worker.stdin, "writable"));
console.log("stdout shape:", streamShape(worker.stdout, "readable"));
console.log("stderr shape:", streamShape(worker.stderr, "readable"));
console.log(
  "diagnostic methods:",
  [
    typeof worker.getHeapStatistics,
    typeof worker.cpuUsage,
    typeof worker.getHeapSnapshot,
    typeof worker.startCpuProfile,
    typeof worker.startHeapProfile,
    typeof worker.performance?.eventLoopUtilization,
  ].join(","),
);

worker.on("message", (message) => {
  console.log(
    "worker surface:",
    [
      message.workerDataValue,
      message.threadName,
      limitsSummary(message.resourceLimits),
      message.envValue,
      message.pathMissing,
    ].join("|"),
  );

  const elu = worker.performance.eventLoopUtilization();
  console.log(
    "elu shape:",
    Object.keys(elu).sort().join(","),
    [typeof elu.active, typeof elu.idle, typeof elu.utilization].join(","),
  );

  Promise.all([
    worker.getHeapStatistics(),
    worker.cpuUsage(),
    worker.startCpuProfile(),
    worker.startHeapProfile(),
  ])
    .then(([heap, cpu, cpuProfile, heapProfile]) => {
      console.log(
        "heap stats:",
        [
          Object.prototype.hasOwnProperty.call(heap, "used_heap_size"),
          typeof heap.used_heap_size,
        ].join(","),
      );
      console.log("cpu usage:", [typeof cpu.user, typeof cpu.system].join(","));
      console.log(
        "profile handles:",
        [typeof cpuProfile.stop, typeof heapProfile.stop].join(","),
      );
      return Promise.all([cpuProfile.stop(), heapProfile.stop()]);
    })
    .then(([cpuProfile, heapProfile]) => {
      console.log(
        "profile stop:",
        [typeof cpuProfile, typeof heapProfile].join(","),
      );
      return worker.terminate();
    })
    .then((code) => {
      console.log("terminate:", typeof code);
    });
});

worker.on("exit", (code) => {
  console.log("exit:", typeof code);
});
