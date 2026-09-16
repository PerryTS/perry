// GC stress with Redis commands in flight (P7).
//
// What has to survive a collection here is the transport's own state: the
// `JsPromise` tokens parked in each connection's pending table, the deferred
// resolutions queued for the main thread, and the owned reply bytes travelling
// inside a `resolve_with` closure. That needs commands *outstanding* while the
// collector runs, not a large JS heap — so this file keeps several replies in
// flight and allocates garbage in a loop between submitting and awaiting them.
//
// The allocating loop is load-bearing, not decoration. Under
// `PERRY_GC_SCHEDULE_RATE=1` the seeded schedule collects at every handled
// safepoint, and a program whose only safepoints are event-loop boundaries
// reaches no back-edge poll at all — the instrument then prints "THIS RUN
// EXERCISED NOTHING WORTH TRUSTING" and exits 70. `scripts/turnloop/apps/redis_parity.ts`
// does exactly that, which is why this is a separate fixture.
//
// Run it stressed and unstressed and compare stdout byte for byte:
//
//   PERRY_GC_DIAG=1 PERRY_GC_SCHEDULE_SEED=7 PERRY_GC_SCHEDULE_RATE=1 \
//   PERRY_GC_SCHEDULE_ALLOC_KB=0 PERRY_GC_PROTECT_FROMSPACE=1 \
//   PERRY_GC_PROTECT_FROMSPACE_DEPTH=800 PERRY_LOOP_STATS=1 ./redis_gc_stress
//
// parity-skip: requires a live Redis fixture
import Redis from "ioredis";

const HOST = process.env.REDIS_HOST ?? "127.0.0.1";
const PORT = Number(process.env.REDIS_PORT ?? "6379");

const a = new Redis({ host: HOST, port: PORT });
const b = new Redis({ host: HOST, port: PORT });
const c = new Redis({ host: HOST, port: PORT });
const d = new Redis({ host: HOST, port: PORT });

// Allocates a fresh object and a fresh string per iteration, so the loop body
// is one codegen proves may allocate and therefore one it emits a back-edge
// poll for. Returns a value so the work cannot be folded away.
function churn(rounds: number): number {
  let total = 0;
  const kept: string[] = [];
  for (let i = 0; i < rounds; i++) {
    const s = `p7-gc-${i}-${"y".repeat(48)}`;
    const o = { i, s, len: s.length };
    total += o.len;
    if (i % 32 === 0) kept.push(s);
  }
  return total + kept.length;
}

async function round(n: number): Promise<string> {
  // Submitted first, awaited last: the collector runs with four replies
  // outstanding and four parked promises the transport must not lose.
  const p0 = a.set(`p7:gc:${n}:0`, `v${n}-0`);
  const p1 = b.set(`p7:gc:${n}:1`, `v${n}-1`);
  const p2 = c.incr(`p7:gc:counter`);
  const p3 = d.get(`p7:gc:${n}:0`);

  const churned = churn(4000);

  const r0 = await p0;
  const r1 = await p1;
  const r2 = await p2;
  await p3;
  const back = await a.get(`p7:gc:${n}:0`);
  // `back` is a string the sink carried as owned bytes and the resolution pump
  // turned into a JS string: comparing its CONTENTS rather than its identity
  // is what makes an evacuation that rewrote the bits visible.
  return `${n}:${r0}:${r1}:${r2 > 0 ? "counted" : "bad"}:${back === `v${n}-0` ? "ok" : "CORRUPT"}:${churned > 0 ? "churned" : "idle"}`;
}

async function main(): Promise<void> {
  await c.del("p7:gc:counter");
  for (let n = 0; n < 6; n++) console.log(await round(n));

  // A rejection must still arrive after all that: the transport owes an answer
  // even when the command fails, and a lost rejection is indistinguishable
  // from a hang.
  let rejected = "no";
  try {
    // INCR on a string key is a server-side error, which comes back as a
    // ReplyError through the same settlement path a success uses.
    await a.set("p7:gc:str", "not-a-number");
    await a.incr("p7:gc:str");
  } catch (e) {
    rejected = e instanceof Error && e.message.length > 0 ? "yes" : "empty";
  }
  console.log("rejection:", rejected);

  for (let n = 0; n < 6; n++) {
    await a.del(`p7:gc:${n}:0`);
    await b.del(`p7:gc:${n}:1`);
  }
  await c.del("p7:gc:counter");
  await a.del("p7:gc:str");

  await a.quit();
  await b.quit();
  await c.quit();
  await d.quit();
  console.log("done");
}

main().catch((e) => {
  console.log("FAILED:", e instanceof Error ? e.message : String(e));
  process.exitCode = 1;
});
