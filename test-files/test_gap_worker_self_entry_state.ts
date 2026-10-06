import { Worker, isMainThread, parentPort, workerData } from "node:worker_threads";
import { leaf, middle } from "./_helpers/worker_self_state.ts";
export const entry = { count: 0, label: "entry" };
if (isMainThread) {
    entry.count = 100;
    leaf.count = 200;
    middle.count = 300;
    for (let id = 1; id <= 2; id++) {
        const replies = await new Promise((resolve) => {
            const w = new Worker(new URL(import.meta.url), { workerData: id });
            const messages: unknown[] = [];
            w.on("message", (m) => {
                messages.push(m);
                if (messages.length === 1) w.postMessage(1);
                else w.terminate().then(() => resolve(messages));
            });
            w.postMessage(1);
        });
        console.log("worker", JSON.stringify(replies));
    }
    console.log("main", entry.count, leaf.count, middle.count);
} else {
    parentPort!.on("message", async (n: number) => {
        entry.count += n;
        leaf.count += n;
        middle.count += n;
        // Re-importing an already evaluated entry must retain its state.
        const again = await import("./test_gap_worker_self_entry_state.ts");
        parentPort!.postMessage([workerData, isMainThread, again.entry.label, again.entry.count,
            leaf.label, leaf.count, middle.label, middle.count]);
    });
}
