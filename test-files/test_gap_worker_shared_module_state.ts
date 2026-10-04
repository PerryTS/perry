// A module imported by the main thread and by workers keeps separate state in
// each thread. The program has no `new Worker` site: it starts its workers
// through the worker_threads namespace value, so the compiler learns about the
// worker only from the URL literal in the helper module.
import { state } from "./_helpers/worker_shared_state.ts";
import { stateWorker } from "./_helpers/worker_shared_state_urls.ts";

const ns = (globalThis as any).process.getBuiltinModule("node:worker_threads");
state.count = 100;

function start(id: number): Promise<unknown> {
    return new Promise((done) => {
        const w = new ns.Worker(stateWorker(), { workerData: id });
        w.once("message", (m: unknown) => {
            w.terminate().then(() => done(m));
        });
    });
}

const replies = await Promise.all([start(1), start(2)]);
for (const reply of replies) console.log("worker", JSON.stringify(reply));
console.log("main", JSON.stringify(state));
