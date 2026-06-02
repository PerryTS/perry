import { Session } from "node:inspector/promises";

async function reportAsync(label: string, fn: () => Promise<unknown> | unknown): Promise<void> {
  try {
    const value = await fn();
    console.log(label, "ok", value === undefined ? "undefined" : typeof value);
  } catch (err) {
    const error = err as { constructor?: { name?: string }, code?: string, message?: string };
    console.log(label, "err", error.constructor?.name, error.code, String(error.message).split("\n")[0]);
  }
}

const session = new Session();
console.log(
  "surface:",
  typeof session.connect,
  typeof session.connectToMainThread,
  typeof session.disconnect,
  typeof session.post,
  typeof session.on,
  typeof session.once,
);

await reportAsync("post before connect:", () => session.post("Runtime.evaluate", {}));
await reportAsync("connectToMainThread:", () => session.connectToMainThread());
await reportAsync("connect:", () => session.connect());

const result = await session.post("Runtime.evaluate", { expression: "21 * 2", returnByValue: true });
console.log(
  "eval promise:",
  result?.result?.type,
  result?.result?.value,
  result?.result?.description,
);

await reportAsync("bad promise:", () => session.post("Nope.nope", {}));

let genericCount = 0;
let specificCount = 0;
let firstGeneric = "";
let firstSpecific = "";
session.on("inspectorNotification", (message: { method?: string }) => {
  genericCount++;
  firstGeneric ||= message?.method || "";
});
session.on("Runtime.consoleAPICalled", (message: { method?: string }) => {
  specificCount++;
  firstSpecific ||= message?.method || "";
});

const enableResult = await session.post("Runtime.enable", {});
console.log("enable promise:", Object.keys(enableResult || {}).length);
await session.post("Runtime.evaluate", { expression: "console.log(\"promise-session-event\")" });
await new Promise((resolve) => setTimeout(resolve, 20));
console.log("events:", genericCount > 0, specificCount > 0, firstGeneric, firstSpecific);

await reportAsync("disconnect:", () => session.disconnect());
await reportAsync("post after disconnect:", () => session.post("Runtime.evaluate", {}));
