import * as workerThreads from "node:worker_threads";

const expected = [
  "BroadcastChannel",
  "MessageChannel",
  "MessagePort",
  "SHARE_ENV",
  "Worker",
  "getEnvironmentData",
  "isInternalThread",
  "isMainThread",
  "isMarkedAsUntransferable",
  "locks",
  "markAsUncloneable",
  "markAsUntransferable",
  "moveMessagePortToContext",
  "parentPort",
  "postMessageToThread",
  "receiveMessageOnPort",
  "resourceLimits",
  "setEnvironmentData",
  "threadId",
  "threadName",
  "workerData",
];

for (const name of expected) {
  const value = (workerThreads as Record<string, any>)[name];
  console.log(
    name,
    Object.prototype.hasOwnProperty.call(workerThreads, name),
    typeof value,
  );
}

console.log("default:", typeof (workerThreads as Record<string, any>).default);
