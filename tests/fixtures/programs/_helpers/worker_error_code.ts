import { Worker, workerData } from "node:worker_threads";
if (workerData === "path") {
    const filename = "bad-worker-path";
    const ns: any = { Worker };
    new ns.Worker(filename);
} else {
    const error: any = new TypeError("worker details");
    error.code = "E_WORKER_TEST";
    throw error;
}
