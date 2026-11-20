import {
  __nativeEventCount,
  getEventsSince,
  subscribe,
  unsubscribe,
  writeSnapshot,
} from "@parcel/watcher";
import { subscribe as sidecarSubscribe } from "@parcel/watcher-darwin-arm64";
import fs from "fs";
import os from "os";
import path from "path";

declare function gc(): void;

type WatchEvent = { path: string; type: "create" | "update" | "delete" };

const root = fs.realpathSync(
  fs.mkdtempSync(path.join(os.tmpdir(), "perry-parcel-watcher-")),
);
const snapshot = path.join(os.tmpdir(), `perry-parcel-watcher-${Date.now()}.json`);
const ignoredDir = path.join(root, "ignored-dir");
const options = {
  // Deliberately unavailable on Linux/Windows: the facade must fall back to
  // the platform default instead of rejecting the subscription.
  backend: "fs-events",
  ignorePaths: [ignoredDir],
  ignoreGlobs: [String.raw`^(?:.*\.ignored)$`],
};
const receivedA: WatchEvent[] = [];
const receivedB: WatchEvent[] = [];
const errors: string[] = [];

const callbackA = (error: Error | null, events: WatchEvent[]) => {
  if (error) errors.push(error.message);
  for (const event of events) receivedA.push(event);
};
const callbackB = (error: Error | null, events: WatchEvent[]) => {
  if (error) errors.push(error.message);
  for (const event of events) receivedB.push(event);
};

const delay = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));
async function waitFor(predicate: () => boolean, message: string) {
  const deadline = Date.now() + 4000;
  while (!predicate() && Date.now() < deadline) await delay(10);
  if (!predicate()) {
    throw new Error(
      `timed out waiting for ${message} (native events: ${__nativeEventCount()})`,
    );
  }
}
const has = (events: WatchEvent[], type: WatchEvent["type"], file: string) =>
  events.some((event) => event.type === type && event.path === file);

await writeSnapshot(root, snapshot, options);
await subscribe(root, callbackA, options);
await subscribe(root, callbackB, options);

// create+update must coalesce to one create in one throttled batch.
const created = path.join(root, "created.txt");
fs.writeFileSync(created, "one");
fs.writeFileSync(created, "two");
await waitFor(() => has(receivedA, "create", created), "create event");
await delay(50);
const createdEvents = receivedA.filter((event) => event.path === created);

// create+delete in one batch must disappear.
const transient = path.join(root, "transient.txt");
fs.writeFileSync(transient, "short-lived");
fs.rmSync(transient);
await delay(100);

// Exercise nested creation, modification, rename ordering, and deletion.
const nestedDir = path.join(root, "nested");
const nested = path.join(nestedDir, "nested.txt");
fs.mkdirSync(nestedDir);
fs.writeFileSync(nested, "nested");
await waitFor(() => has(receivedA, "create", nested), "nested create event");

fs.writeFileSync(created, "three");
await waitFor(() => has(receivedA, "update", created), "update event");

const renamed = path.join(root, "renamed.txt");
fs.renameSync(created, renamed);
await waitFor(
  () => has(receivedA, "delete", created) && has(receivedA, "create", renamed),
  "rename events",
);
const renameDelete = receivedA.findIndex(
  (event) => event.type === "delete" && event.path === created,
);
const renameCreate = receivedA.findIndex(
  (event) => event.type === "create" && event.path === renamed,
);

fs.rmSync(renamed);
await waitFor(() => has(receivedA, "delete", renamed), "delete event");

fs.mkdirSync(ignoredDir);
fs.writeFileSync(path.join(ignoredDir, "ignored.txt"), "ignored");
const ignoredGlob = path.join(root, ".dot.ignored");
fs.writeFileSync(ignoredGlob, "ignored");
await delay(100);

// Queue an event immediately before unsubscribe, then force a collection.
// Once the promise resolves neither that in-flight event nor a later event may
// reach callback A, while callback B must remain independently subscribed.
const inFlight = path.join(root, "in-flight.txt");
fs.writeFileSync(inFlight, "queued");
await unsubscribe(root, callbackA, options);
if (typeof gc === "function") gc();
const countAfterUnsubscribe = receivedA.length;
const late = path.join(root, "late.txt");
fs.writeFileSync(late, "late");
await waitFor(() => has(receivedB, "create", late), "second subscription event");
await delay(100);
await unsubscribe(root, callbackB, options);

const diff = await getEventsSince(root, snapshot, options);
const checks = [
  typeof sidecarSubscribe === "function",
  errors.length === 0,
  createdEvents.length === 1 && createdEvents[0].type === "create",
  !receivedA.some((event) => event.path === transient),
  has(receivedA, "create", nested),
  renameDelete >= 0 && renameDelete < renameCreate,
  has(receivedA, "delete", renamed),
  !receivedA.some((event) => event.path.startsWith(ignoredDir)),
  !receivedA.some((event) => event.path === ignoredGlob),
  receivedA.length === countAfterUnsubscribe,
  has(receivedB, "create", late),
  diff.some((event) => event.path === nested),
  receivedA.every((event) => path.isAbsolute(event.path)),
  __nativeEventCount() > 0,
];
for (const check of checks) console.log(check);
if (checks.some((check) => !check)) {
  throw new Error("@parcel/watcher facade integration check failed");
}

fs.rmSync(root, { recursive: true, force: true });
fs.rmSync(snapshot, { force: true });
