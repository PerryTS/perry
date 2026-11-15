// worker_threads use: init flags are thread-local, no image (cut 0).
import { isMainThread } from "node:worker_threads";
gc();
console.log("K2B_FW", isMainThread);
