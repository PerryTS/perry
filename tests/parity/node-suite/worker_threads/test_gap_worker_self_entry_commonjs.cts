// CommonJS provides the real __filename binding, without an ESM shim.
const { Worker, isMainThread, parentPort } = require("node:worker_threads");
if (isMainThread) {
    const w = new Worker(__filename);
    w.on("message", (m: number) => { console.log("commonjs", m); w.terminate(); });
    w.postMessage(21);
} else {
    if (require.main !== module) throw new Error("wrong worker main module");
    parentPort.on("message", (n: number) => parentPort.postMessage(n * 2));
}
