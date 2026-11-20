import { parentPort } from "node:worker_threads";
parentPort!.postMessage("ready");
let counter = 0;
while (true) {
  counter = (counter + 1) | 0;
}
