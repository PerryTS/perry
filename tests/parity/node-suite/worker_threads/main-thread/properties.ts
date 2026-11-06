import * as worker_threads from "node:worker_threads";

console.log("isMainThread:", worker_threads.isMainThread);
console.log("isInternalThread:", worker_threads.isInternalThread);
console.log("parentPort:", worker_threads.parentPort);
console.log("threadId:", worker_threads.threadId);
console.log("threadName JSON:", JSON.stringify(worker_threads.threadName));
console.log("workerData:", worker_threads.workerData);
console.log("resourceLimits keys:", Object.keys(worker_threads.resourceLimits).length);
console.log("SHARE_ENV:", String(worker_threads.SHARE_ENV));
