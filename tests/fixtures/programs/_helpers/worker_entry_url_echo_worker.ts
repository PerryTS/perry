import { parentPort, workerData } from "node:worker_threads";

parentPort!.postMessage({ hello: workerData });
parentPort!.on("message", (m: string) => parentPort!.postMessage("echo:" + m));
