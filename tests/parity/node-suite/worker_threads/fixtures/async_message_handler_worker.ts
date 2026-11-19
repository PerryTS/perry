// Worker side of test_gap_10394_async_message_handler_resumes.ts.
import { parentPort } from "node:worker_threads";

parentPort!.on("message", async (kind: string) => {
  if (kind === "microtask") await Promise.resolve();
  if (kind === "timer") await new Promise((r) => setTimeout(r, 10));
  if (kind === "timer2") {
    await new Promise((r) => setTimeout(r, 1));
    await new Promise((r) => setTimeout(r, 1));
  }
  if (kind === "io") {
    const fs = await import("node:fs/promises");
    await fs.stat(".");
  }
  parentPort!.postMessage(kind);
});
