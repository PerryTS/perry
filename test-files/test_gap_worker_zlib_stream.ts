// zlib streams belong to the thread that made them. A worker inflating a
// stream while the main thread inflates its own must get all of its output:
// the stream events used to sit in one process-wide queue that whichever
// thread pumped first drained, so the main thread took the worker's data
// events (and ran its closures on the wrong heap) and the worker's inflate
// came back short.
import { Worker } from "node:worker_threads";
import { inflatedBytes, payload } from "./_helpers/worker_zlib_stream_lib.ts";

const guard = setTimeout(() => {
    console.log("timeout");
    process.exit(2);
}, 20000);

const w = new Worker(new URL("./_helpers/worker_zlib_stream_worker.ts", import.meta.url));
const replies: any[] = [];
let wake: (() => void) | null = null;
w.on("message", (m: any) => {
    replies.push(m);
    if (wake) wake();
});
const next = (): Promise<any> =>
    new Promise((resolve) => {
        if (replies.length) return resolve(replies.shift());
        wake = () => {
            wake = null;
            resolve(replies.shift());
        };
    });

for (let round = 0; round < 4; round++) {
    w.postMessage(round);
    const mine = await inflatedBytes(payload(round + 11));
    const theirs = await next();
    console.log("round", round, "main", mine, "worker", theirs.round, theirs.bytes);
}
await w.terminate();
clearTimeout(guard);
console.log("done");
