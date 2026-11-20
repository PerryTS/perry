import { parentPort, workerData } from "node:worker_threads";

export interface Reply {
    ok: boolean;
    echo: string;
}

const reply: Reply = { ok: true, echo: String(workerData) };
parentPort!.postMessage(reply);
