import { parentPort, workerData, isMainThread, threadId } from "node:worker_threads";

// The namespace as a value, the way a program reaches it lazily.
const ns = (globalThis as any).process.getBuiltinModule("node:worker_threads");
parentPort!.postMessage({
    workerData,
    isMainThread,
    nsIsMainThread: ns.isMainThread,
    threadIdSet: threadId > 0,
    nsThreadIdSame: ns.threadId === threadId,
});
parentPort!.on("message", (m: string) => parentPort!.postMessage("echo:" + m));
