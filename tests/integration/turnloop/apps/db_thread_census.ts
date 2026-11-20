// P7's headline claim, measured: N database connections no longer mean N
// parked OS threads.
//
// Opens 16 independent Redis clients against one server and keeps a command in
// flight on every one of them at once, reporting the process's thread count and
// thread NAMES at each stage. Names matter: a count alone cannot tell a tokio
// blocking-pool thread from anything else, and `tokio-rt-worker x16` in the
// in-flight row is exactly the old shape.
//
// Before P7 each in-flight command borrowed one thread from tokio's blocking
// pool for the whole round trip (`spawn_blocking` + `Handle::block_on`), so the
// in-flight row grew with the concurrency. After P7 the connections are loop
// state and the row does not move.
//
// The clients are 16 separate `const`s rather than an array on purpose: a method
// call whose receiver is an array element (`clients[i].set(...)`) does not
// resolve to Perry's native-method table and silently returns `undefined`, on
// the base commit as well as on this branch. Writing them out keeps this file
// about the transport rather than about that codegen limitation.
//
// Perry reads REDIS_HOST / REDIS_PORT / REDIS_TLS from the environment; Node's
// ioredis reads the constructor argument. Set both:
//
//   REDIS_HOST=127.0.0.1 REDIS_PORT=56379 REDIS_TLS=false
//
// parity-skip: requires a live Redis fixture
import { readdirSync, readFileSync } from "node:fs";
import Redis from "ioredis";

const HOST = process.env.REDIS_HOST ?? "127.0.0.1";
const PORT = Number(process.env.REDIS_PORT ?? "6379");

function names(): string {
  try {
    const counts = new Map<string, number>();
    for (const t of readdirSync("/proc/self/task")) {
      try {
        const n = readFileSync(`/proc/self/task/${t}/comm`, "utf8").trim();
        counts.set(n, (counts.get(n) ?? 0) + 1);
      } catch {}
    }
    const out: string[] = [];
    for (const [n, c] of counts) out.push(`${n} x${c}`);
    out.sort();
    return out.join(", ");
  } catch {
    return "unavailable";
  }
}

function count(): number {
  try {
    return readdirSync("/proc/self/task").length;
  } catch {
    return -1;
  }
}

const c0 = new Redis({ host: HOST, port: PORT });
const c1 = new Redis({ host: HOST, port: PORT });
const c2 = new Redis({ host: HOST, port: PORT });
const c3 = new Redis({ host: HOST, port: PORT });
const c4 = new Redis({ host: HOST, port: PORT });
const c5 = new Redis({ host: HOST, port: PORT });
const c6 = new Redis({ host: HOST, port: PORT });
const c7 = new Redis({ host: HOST, port: PORT });
const c8 = new Redis({ host: HOST, port: PORT });
const c9 = new Redis({ host: HOST, port: PORT });
const c10 = new Redis({ host: HOST, port: PORT });
const c11 = new Redis({ host: HOST, port: PORT });
const c12 = new Redis({ host: HOST, port: PORT });
const c13 = new Redis({ host: HOST, port: PORT });
const c14 = new Redis({ host: HOST, port: PORT });
const c15 = new Redis({ host: HOST, port: PORT });

async function main(): Promise<void> {
  console.log("connections:", 16);
  console.log("idle threads:", count());

  // Every command submitted before any is awaited: this is the moment the old
  // transport needed 16 blocking threads at once.
  const inflight = [
  c0.set("p7:census:0", "v0"),
  c1.set("p7:census:1", "v1"),
  c2.set("p7:census:2", "v2"),
  c3.set("p7:census:3", "v3"),
  c4.set("p7:census:4", "v4"),
  c5.set("p7:census:5", "v5"),
  c6.set("p7:census:6", "v6"),
  c7.set("p7:census:7", "v7"),
  c8.set("p7:census:8", "v8"),
  c9.set("p7:census:9", "v9"),
  c10.set("p7:census:10", "v10"),
  c11.set("p7:census:11", "v11"),
  c12.set("p7:census:12", "v12"),
  c13.set("p7:census:13", "v13"),
  c14.set("p7:census:14", "v14"),
  c15.set("p7:census:15", "v15")
  ];
  console.log("in-flight threads:", count(), "|", names());
  const acks = await Promise.all(inflight);
  console.log("acks:", acks.length, acks.every((a) => a === "OK") ? "all-OK" : "MISMATCH");

  // A second round, so the reading is not a one-shot artefact of the connects.
  const reads = await Promise.all([
  c0.get("p7:census:0"),
  c1.get("p7:census:1"),
  c2.get("p7:census:2"),
  c3.get("p7:census:3"),
  c4.get("p7:census:4"),
  c5.get("p7:census:5"),
  c6.get("p7:census:6"),
  c7.get("p7:census:7"),
  c8.get("p7:census:8"),
  c9.get("p7:census:9"),
  c10.get("p7:census:10"),
  c11.get("p7:census:11"),
  c12.get("p7:census:12"),
  c13.get("p7:census:13"),
  c14.get("p7:census:14"),
  c15.get("p7:census:15")
  ]);
  let correct = 0;
  for (let i = 0; i < reads.length; i++) if (reads[i] === `v${i}`) correct++;
  console.log("reads:", correct, "of", 16);
  console.log("after threads:", count(), "|", names());

  await Promise.all([
  c0.del("p7:census:0"),
  c1.del("p7:census:1"),
  c2.del("p7:census:2"),
  c3.del("p7:census:3"),
  c4.del("p7:census:4"),
  c5.del("p7:census:5"),
  c6.del("p7:census:6"),
  c7.del("p7:census:7"),
  c8.del("p7:census:8"),
  c9.del("p7:census:9"),
  c10.del("p7:census:10"),
  c11.del("p7:census:11"),
  c12.del("p7:census:12"),
  c13.del("p7:census:13"),
  c14.del("p7:census:14"),
  c15.del("p7:census:15")
  ]);
  await Promise.all([
  c0.quit(),
  c1.quit(),
  c2.quit(),
  c3.quit(),
  c4.quit(),
  c5.quit(),
  c6.quit(),
  c7.quit(),
  c8.quit(),
  c9.quit(),
  c10.quit(),
  c11.quit(),
  c12.quit(),
  c13.quit(),
  c14.quit(),
  c15.quit()
  ]);
  console.log("closed threads:", count());
  console.log("done");
}

main().catch((e) => {
  console.log("FAILED:", e instanceof Error ? e.message : String(e));
  process.exitCode = 1;
});
