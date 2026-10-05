import * as workers from "node:worker_threads";
import { parentPort, workerData } from "node:worker_threads";
if (workers.workerData !== workers.workerData) process.exit(6);
parentPort!.postMessage(new Uint8Array(workerData)[0]);
parentPort!.on("message", (m: any) => {
    if (m.own) { parentPort!.postMessage(m, [m.own.buffer]); return; }
    if (m.u8.buffer !== m.ab || m.offset.buffer !== m.ab || m.u16.buffer !== m.ab || m.dv.buffer !== m.ab) process.exit(4);
    m.offset[0] = 77;
    parentPort!.postMessage(m, [m.ab]);
    if (m.ab.byteLength !== 0 || m.offset.length !== 0 || m.u16.length !== 0) process.exit(5);
});
