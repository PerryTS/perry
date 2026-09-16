// The Worker half of `p9_agent_loop_rss.ts`. It does exactly enough network
// work to force its agent's loop to the NET profile (4096 handles, 8192
// operations, 64 x 16 KiB pooled buffers), then idles until the parent exits --
// so the parent's RSS reading covers N loops that are alive and idle.
//
// The result is written to a FILE, never posted: with more than one Worker a
// `postMessage` to the parent is not reliably delivered on either arm (a
// pre-existing defect, `p9_worker_message_fanin.ts`), and a measurement that
// never starts is worse than one that is wrong.
//
// It is REPORTED, never swallowed. An agent whose fetch failed never upgraded
// its loop to the NET profile, so it is not one of the loops the number is
// supposed to be measuring -- and a row of 64 agents where 64 fetches failed
// would otherwise read as "an agent loop is free".
import { writeFileSync } from "node:fs";
import { workerData } from "node:worker_threads";

const url = process.env.P9_URL ?? "http://127.0.0.1:8099/";
const mode = process.env.P9_RSS_MODE ?? "net";
const data = (workerData ?? {}) as { index?: number; readyDir?: string };
const index = data.index ?? 0;
const readyDir = data.readyDir ?? "/tmp/p9-rss-ready";

let netStatus = "skipped";
if (mode === "net") {
  try {
    const r = await fetch(url);
    await r.text();
    netStatus = `ok:${r.status}`;
  } catch (e) {
    netStatus = "error:" + (e as Error).message;
  }
}

writeFileSync(`${readyDir}/${index}.ready`, netStatus);

// Stay alive and idle, holding this agent's loop, until the parent has taken
// its reading and exits the process out from under us.
setInterval(() => {}, 1000);
