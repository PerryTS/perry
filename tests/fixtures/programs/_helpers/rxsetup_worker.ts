// Worker body for test_gap_rxsetup_worker.ts.
import { parentPort, workerData } from "node:worker_threads";

const out: string[] = [];
const log: string[] = [];
const cases = (label: string) => {
  for (const s of ["abc", "a+b+c"]) {
    const g = /\+/g;
    const n = /\+/;
    g.lastIndex = 3;
    out.push(label + " [" + s + "] " + JSON.stringify([s.replace(g, "_"), g.lastIndex, s.replace(n, "_"), log.splice(0)]));
  }
};
cases("before");
if (workerData === "patched") {
  const exec = RegExp.prototype.exec;
  RegExp.prototype.exec = function (this: RegExp, s: string) { log.push("exec " + s); return exec.call(this, s); };
  cases("exec");
  RegExp.prototype.exec = exec;
  const repl = Object.getOwnPropertyDescriptor(RegExp.prototype, Symbol.replace)!;
  Object.defineProperty(RegExp.prototype, Symbol.replace, {
    configurable: true, writable: true,
    value: function (s: string) { log.push("replace " + s); return "patched:" + s; },
  });
  cases("symbol");
  Object.defineProperty(RegExp.prototype, Symbol.replace, repl);
  cases("restored");
}
parentPort!.postMessage(out.join(" ## "));
