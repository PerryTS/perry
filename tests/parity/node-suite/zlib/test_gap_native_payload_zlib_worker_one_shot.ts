// One-shot zlib callbacks queued from a Worker run on that Worker's own loop:
// zlib.gzip/gunzip/deflate callbacks and a promisified one-shot, then a second
// Worker started after the first one exited (its arena may reuse the first
// one's addresses), then a main-thread one-shot after both are gone.
import { Worker, isMainThread, parentPort, workerData } from "node:worker_threads";
import * as zlib from "node:zlib";
import { promisify } from "node:util";

if (isMainThread) {
    const run = (round: number) =>
        new Promise<void>((resolve) => {
            const w = new Worker(new URL(import.meta.url), { workerData: { round } });
            w.on("message", (m) => console.log("worker", round, m));
            w.on("error", (e) => console.log("worker error", round, (e as Error).message));
            w.on("exit", (code) => {
                console.log("exit", round, code);
                resolve();
            });
        });
    await run(1);
    await run(2);
    // The main thread's own one-shot after both workers are gone.
    zlib.gzip("main after workers", (err, out) => {
        console.log("main", err === null, zlib.gunzipSync(out).toString());
    });
} else {
    const round = workerData.round as number;
    const text = `worker ${round} payload `.repeat(40);
    await new Promise<void>((resolve) => {
        zlib.gzip(text, (err, gz) => {
            if (err) throw err;
            zlib.gunzip(gz, (err2, back) => {
                parentPort!.postMessage(`gzip ${err2 === null} ${back.toString() === text}`);
                resolve();
            });
        });
    });
    await new Promise<void>((resolve) => {
        zlib.deflate(Buffer.from(text), { level: 1 }, (err, out) => {
            parentPort!.postMessage(`deflate ${err === null} ${zlib.inflateSync(out).toString() === text}`);
            resolve();
        });
    });
    const brotli = await promisify(zlib.brotliCompress)(Buffer.from(text));
    parentPort!.postMessage(`brotli ${zlib.brotliDecompressSync(brotli).toString() === text}`);
}
