import { parentPort, workerData } from "node:worker_threads";
declare function gc(): void;
function collect(): void { if (typeof gc === "function") gc(); }
const funcs: any[] = [Object.keys, Math.max, Array.isArray];
const kept: any[] = [];
async function step(n: number): Promise<void> {
  const fn = funcs[n % funcs.length];
  Object.defineProperty(fn, "name", {value: "worker-" + n, writable: true, enumerable: true, configurable: true});
  Object.defineProperty(fn, "extra" + n, {value: n, writable: true, enumerable: true, configurable: true});
  const bytes = new Uint8Array(20000);
  bytes[0] = n % 251;
  kept.push(bytes);
  if (kept.length > 48) kept.shift();
  if (n % 8 === 0) collect();
  await null;
  const d: any = Object.getOwnPropertyDescriptor(fn, "name");
  if (fn.name !== "worker-" + n || d.value !== fn.name || !d.writable || !d.enumerable || !d.configurable || fn["extra" + n] !== n) throw new Error("worker function attributes lost");
  // Exit between descriptor operations with the last function's update unfinished.
  if (workerData === "midway" && n === 80) throw new Error("midway exit");
  Object.defineProperty(fn, "length", {value: n % 7, writable: true, enumerable: true, configurable: true});
  if (n + 1 < 160) return step(n + 1);
}
step(0).then(() => {
  parentPort!.postMessage("worker attributes ok");
});
