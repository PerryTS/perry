import { parentPort } from "node:worker_threads";
const store = new ArrayBuffer(32 * 1024 * 1024);
const bytes = new Uint8Array(store);
for (let i = 0; i < bytes.length; i += 4096) bytes[i] = (i / 4096) & 255;
bytes[bytes.length - 1] = 199;
parentPort!.postMessage(store, [store]);
if (store.byteLength !== 0) process.exit(7);
// With no listeners or pending work, reaching the end exits this worker.
