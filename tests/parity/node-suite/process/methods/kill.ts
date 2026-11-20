// process.kill maps signal names/numbers and rejects invalid signal inputs.
import { spawn } from "node:child_process";

console.log("is function:", typeof process.kill === "function");

function errorCode(err: unknown): string {
  const anyErr = err as { code?: string };
  return typeof anyErr.code === "string" ? anyErr.code : "no-code";
}

async function probe(label: string, signal: unknown, omitted = false): Promise<void> {
  const child = spawn(process.execPath, ["-e", "setInterval(() => {}, 1000)"], {
    stdio: "ignore",
  });
  await new Promise((resolve) => setTimeout(resolve, 40));
  try {
    const result = omitted
      ? process.kill(child.pid)
      : process.kill(child.pid, signal as never);
    console.log("kill:", label, "OK", result);
  } catch (err) {
    console.log("kill:", label, "THROW", (err as Error).name, errorCode(err));
  }
  try {
    process.kill(child.pid, "SIGKILL");
  } catch (_err) {
    // The child may already have exited after a terminating signal.
  }
  await new Promise((resolve) => child.once("exit", resolve));
}

await probe("omitted", undefined, true);
await probe("undefined", undefined);
await probe("null", null);
await probe("SIGTERM", "SIGTERM");
await probe("TERM", "TERM");
await probe("sigterm", "sigterm");
await probe("numeric-string-zero", "0");
await probe("BOGUS", "BOGUS");
await probe("zero", 0);
await probe("fifteen", 15);
await probe("fraction", 1.5);
await probe("nan", NaN);
await probe("infinity", Infinity);
await probe("boolean", true);
await probe("object", {});
await probe("array", []);
