// The Worker half of `p9_worker_message_fanin.ts`: post once, return.
import { parentPort } from "node:worker_threads";

parentPort?.postMessage("ready");
