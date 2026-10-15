// No-match and match replaces inside a worker, then a RegExp.prototype
// override inside that worker: the shadow state is per worker, so main's
// pristine prototype and the worker's patched one must not leak into each
// other.
import { Worker } from "node:worker_threads";

const url = new URL("./_helpers/rxsetup_worker.ts", import.meta.url);
const show = (label: string, value: unknown) => console.log(label, JSON.stringify(value));

function run(role: string): Promise<void> {
  return new Promise((resolve) => {
    const w = new Worker(url, { workerData: role });
    const events: string[] = [];
    w.on("message", (m: string) => events.push(m));
    w.on("error", (e: Error) => events.push("error " + e.message));
    w.on("exit", (code: number) => {
      console.log(role + ": " + events.join(" | ") + "; exit " + code);
      resolve();
    });
  });
}

const mainCase = (label: string) => {
  const g = /\+/g;
  show(label, ["abc".replace(g, "_"), g.lastIndex, "a+b".replace(/\+/g, "_"), "a+b".replace(/\+/, "_")]);
};
mainCase("main before");
await run("plain");
mainCase("main between");
await run("patched");
mainCase("main after");
await run("plain");
