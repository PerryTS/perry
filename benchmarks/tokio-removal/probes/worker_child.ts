import { parentPort, workerData } from "node:worker_threads";
setTimeout(() => parentPort!.postMessage(workerData.n * 2), 5);
