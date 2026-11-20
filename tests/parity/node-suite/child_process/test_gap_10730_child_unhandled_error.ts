// #10730 — a ChildProcess `error` event with no listener must throw, as every
// Node EventEmitter does. Perry dropped it: a failed spawn of a missing binary
// looked like a child that ran, `close` fired, and the program carried on. That
// is how test_gap_9592's missing `/bin/true` on macOS read as a Perry pass
// while Node died with `Unhandled 'error' event`.
//
// The throw is observed through `uncaughtException` so the result is printed
// rather than inferred from an exit status.
import { spawn } from "node:child_process";

// Absolute, and absent on every platform the suite runs on.
const MISSING = "/nonexistent-perry-10730/true";

const events: string[] = [];
process.on("uncaughtException", (err: any) => {
  events.push(`uncaught ${err.code} ${err.syscall} ${err instanceof Error}`);
});

// 1. With a listener: the error is delivered, nothing is thrown, `close` fires.
await new Promise<void>((resolve) => {
  const child = spawn(MISSING, [], { stdio: "ignore" });
  child.on("error", (err: any) => events.push(`handled ${err.code} ${err.path}`));
  child.on("close", (code: any) => {
    events.push(`close ${code}`);
    resolve();
  });
});

// 2. Without one: the error is thrown, and `close` never fires.
{
  const child = spawn(MISSING, ["a"], { stdio: "ignore" });
  child.on("close", () => events.push("close without listener"));
}

// 3. A listener that was removed again is no listener.
{
  const child = spawn(MISSING, ["b"], { stdio: "ignore" });
  const onError = () => events.push("removed listener ran");
  child.on("error", onError);
  child.removeListener("error", onError);
}

setTimeout(() => {
  for (const line of events) console.log(line);
}, 200);
