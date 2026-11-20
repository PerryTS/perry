import { parentPort, workerData } from "node:worker_threads";
import { describe } from "./clone_describe.ts";

parentPort!.postMessage({ seen: "workerData " + describe(workerData) });
parentPort!.on("message", (m: any) => {
    if (m && m.cmd === "post-fn") {
        try {
            parentPort!.postMessage({ f: () => 1 });
            parentPort!.postMessage({ seen: "worker posted a function" });
        } catch (e: any) {
            parentPort!.postMessage({ seen: "worker post-fn " + e.name });
        }
        return;
    }
    if (m && m.cmd === "send-ab") {
        const ab = new ArrayBuffer(4);
        new Uint8Array(ab).set([9, 8, 7, 6]);
        parentPort!.postMessage({ seen: "worker transfer", back: ab }, [ab]);
        parentPort!.postMessage({ seen: "worker after transfer byteLength=" + ab.byteLength });
        return;
    }
    parentPort!.postMessage({ seen: describe(m), back: m });
});
