// The Worker half of `p9_agent_loop_rss.ts`. It does exactly enough network
// work to force its agent's loop to the NET profile (4096 handles, 8192
// operations, 64 x 16 KiB pooled buffers), then idles until the parent says
// stop — so the parent's RSS reading covers N loops that are alive and idle,
// which is the number the brief asks for.
import { parentPort } from "node:worker_threads";

const url = process.env.P9_URL ?? "http://127.0.0.1:8099/";
const mode = process.env.P9_RSS_MODE ?? "net";

if (mode === "net") {
  try {
    const r = await fetch(url);
    await r.text();
  } catch {
    // Reported by the parent as a missing-loop row rather than swallowed: the
    // measurement is only meaningful if the loop really reached NET.
  }
}

parentPort?.postMessage("ready");
await new Promise<void>((resolve) => {
  parentPort?.on("message", (m: unknown) => {
    if (m === "stop") resolve();
  });
});
