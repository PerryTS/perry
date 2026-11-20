import { parentPort, workerData } from "node:worker_threads";

const mode: string = workerData;
if (mode === "init-throw") throw new RangeError("boom in module init");
parentPort!.on("message", (m: string) => {
    parentPort!.postMessage("got " + m);
    if (mode === "handler-throw") throw new Error("boom in handler");
    if (mode === "handler-throw-string") throw "plain string";
    if (mode === "rejection") Promise.reject(new TypeError("async boom"));
    if (mode === "async-handler") {
        (async () => {
            await null;
            throw new SyntaxError("boom after await");
        })();
    }
    if (mode === "caught") {
        try {
            throw new Error("caught inside");
        } catch (e: any) {
            parentPort!.postMessage("caught " + e.message);
        }
    }
});
