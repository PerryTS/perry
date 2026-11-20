import { parentPort, workerData } from "node:worker_threads";
import { state } from "./worker_shared_state.ts";

state.count++;
parentPort!.postMessage({ id: workerData, count: state.count, inits: state.inits });
