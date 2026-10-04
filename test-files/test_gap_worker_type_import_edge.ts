// `import type` from a worker module plus `new Worker(new URL(...))` of the
// same module in one file: the type import is erased, and the Worker target
// must still be compiled into the binary as a worker entry.
import type { Reply } from "./_helpers/worker_type_import_edge_worker.ts";
import { Worker } from "node:worker_threads";

const w = new Worker(new URL("./_helpers/worker_type_import_edge_worker.ts", import.meta.url), {
    workerData: "typed",
});
w.once("message", (reply: Reply) => {
    console.log("reply:", reply.ok, reply.echo);
    void w.terminate();
});
