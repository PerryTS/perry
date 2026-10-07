// parity-node-argv: --expose-gc
// Read transferred bytes only after the worker has exited and its arena died.
import { Worker } from "node:worker_threads";
const guard = setTimeout(() => process.exit(2), 60000);
let received: any = null;
let messages = 0;
const worker = new Worker(new URL("./_helpers/region_transfer_sender_exit.ts", import.meta.url));
worker.on("message", (value: any) => { received = value; messages++; });
worker.on("exit", (code: number) => {
    if (code !== 0 || messages !== 1 || !received) process.exit(3);
    const bytes = new Uint8Array(received);
    if (bytes.length !== 32 * 1024 * 1024) process.exit(4);
    // Churn the receiver after sender teardown, before reading the payload.
    for (let i = 0; i < 12; i++) {
        const churn = Buffer.alloc(2 * 1024 * 1024, i);
        if (churn[0] !== i) process.exit(5);
    }
    if (typeof globalThis.gc === "function") globalThis.gc();
    let sum = 0;
    for (let i = 0; i < bytes.length; i += 4096) sum += bytes[i];
    console.log("after sender exit", bytes.length, sum, bytes[bytes.length - 1]);
    if (sum !== 1044480 || bytes[bytes.length - 1] !== 199) process.exit(6);
    clearTimeout(guard);
});
