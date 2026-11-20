// #12165: a plain Worker that does some work and returns must deliver its own
// 'exit' event to the parent. Awaiting it five times in a row makes a missed
// delivery hang the program instead of passing by luck.
import { Worker, isMainThread, parentPort } from "node:worker_threads";
import { once } from "node:events";
if (isMainThread) {
  for (let i = 0; i < 5; i++) {
    const w = new Worker(new URL(import.meta.url));
    const exited = once(w, "exit");
    const [msg] = await once(w, "message");
    const [code] = await exited;
    console.log(i, msg, code);
  }
} else {
  let s = 0;
  for (let i = 0; i < 100000; i++) s += i;
  parentPort!.postMessage(s);
}
