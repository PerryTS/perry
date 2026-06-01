import * as workerThreads from "node:worker_threads";

const workerThreadsAny = workerThreads as Record<string, any>;

console.log("workerData own:", Object.prototype.hasOwnProperty.call(workerThreads, "workerData"));
console.log("workerData in namespace:", "workerData" in workerThreads);
console.log("workerData type/value:", typeof workerThreads.workerData, workerThreads.workerData);
console.log("workerData key:", Object.keys(workerThreads).includes("workerData"));

console.log(
  "getWorkerData own/in/type:",
  Object.prototype.hasOwnProperty.call(workerThreads, "getWorkerData"),
  "getWorkerData" in workerThreads,
  typeof workerThreadsAny.getWorkerData,
);
console.log("getWorkerData key:", Object.keys(workerThreads).includes("getWorkerData"));

try {
  (workerThreads as any).workerData();
  console.log("workerData call: ok");
} catch (error: any) {
  console.log(
    "workerData call error:",
    error?.name,
    error?.code ?? "",
    String(error?.message).includes("not a function"),
  );
}
