import { Worker } from "node:worker_threads";
const w = new Worker(new URL("./worker.ts", import.meta.url));
let received = 0;
const guard = setTimeout(() => process.exit(2), 60000);
w.on("message", (ab: ArrayBuffer) => {
    const a = new Uint8Array(ab);
    if (a.length !== 8 * 1024 * 1024 || a[0] !== (received & 255) || a[a.length - 1] !== (received & 255)) process.exit(3);
    received++;
    if (received === 100) {
        console.log("100 transfers, bytes and detach verified");
        clearTimeout(guard);
        w.terminate();
    } else send();
});
function send() {
    const ab = new ArrayBuffer(8 * 1024 * 1024);
    const a = new Uint8Array(ab);
    a.fill(received & 255);
    w.postMessage(ab, [ab]);
    if (ab.byteLength !== 0 || a.length !== 0) process.exit(4);
}
send();
