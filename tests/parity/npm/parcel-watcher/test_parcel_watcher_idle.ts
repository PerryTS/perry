// parity-skip: benchmark fixture requires PERRY_WATCH_ROOT/PERRY_WATCH_READY and the dedicated Parcel watcher harness
import { subscribe, unsubscribe } from "@parcel/watcher";
import fs from "fs";
import path from "path";

const configuredRoot = process.env.PERRY_WATCH_ROOT;
const readyPath = process.env.PERRY_WATCH_READY;
if (!configuredRoot || !readyPath) {
  throw new Error(
    "PERRY_WATCH_ROOT and PERRY_WATCH_READY are required for the idle benchmark",
  );
}
const root = fs.realpathSync(configuredRoot);
const wakePath = path.join(root, "wake.txt");

const options = {};
let callbacks = 0;
let finish: () => void = () => {};
const wake = new Promise<void>((resolve) => {
  finish = resolve;
});
const callback = (_error: Error | null, events: { path: string }[]) => {
  callbacks++;
  if (events.some((event) => event.path === wakePath)) finish();
};
await subscribe(root, callback, options);
fs.writeFileSync(readyPath, "ready");
const before = process.cpuUsage();
const started = Date.now();
await wake;
const idle = process.cpuUsage(before);
const elapsed = Date.now() - started;
await unsubscribe(root, callback, options);

console.log(callbacks === 1);
console.log(`idle CPU: ${(idle.user + idle.system) / 1000} ms / ${elapsed} ms`);
