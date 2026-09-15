// GC stress subject for turnloop P4: pool jobs in flight while the collector
// runs at every handled safepoint.
//
// Deliberately small JS-side loops: PERRY_GC_SCHEDULE_RATE=1 collects at every
// poll, so a multi-million-iteration fill loop would not finish. What has to be
// live across those collections is the pool's own state — the parked callbacks,
// the promises, the deferred resolutions — and that needs jobs in flight, not
// a large JS heap.
import { pbkdf2, scrypt } from "node:crypto";
import { gzip, gunzip } from "node:zlib";
import { promisify } from "node:util";

const pbkdf2Async = promisify(pbkdf2);
const scryptAsync = promisify(scrypt);
const gzipAsync = promisify(gzip);
const gunzipAsync = promisify(gunzip);

async function main(): Promise<void> {
  const chunk = Buffer.alloc(64 * 1024, 7);

  // A burst: more jobs than the pool has threads, so some are still queued
  // while collections run.
  const burst: Promise<unknown>[] = [];
  for (let i = 0; i < 6; i++) {
    burst.push(pbkdf2Async("perry" + i, "salt", 20_000, 32, "sha256"));
    burst.push(gzipAsync(chunk));
  }
  // Allocate JS garbage while the jobs are out, so the collector has work.
  const garbage: string[] = [];
  for (let i = 0; i < 400; i++) garbage.push("g" + i + ":" + i.toString(16));
  const results = await Promise.all(burst);
  console.log("burst results:", results.length);
  console.log("garbage kept:", garbage.length);
  console.log(
    "every pbkdf2 is 32 bytes:",
    results.filter((_, i) => i % 2 === 0).every((b) => (b as Buffer).length === 32),
  );

  // A round trip whose correctness would break if a parked callback moved.
  const packed = (await gzipAsync(chunk)) as Buffer;
  const back = (await gunzipAsync(packed)) as Buffer;
  console.log("round trip byte-identical:", back.equals(chunk));

  const derived = (await scryptAsync("perry", "turnloop", 32, {
    N: 1024,
    r: 8,
    p: 1,
  })) as Buffer;
  console.log("scrypt length:", derived.length);

  // A failure path under stress: the rejection must still arrive.
  let threw = "none";
  try {
    await gunzipAsync(Buffer.from([1, 2, 3, 4]));
  } catch {
    threw = "threw";
  }
  console.log("gunzip of garbage:", threw);
}

main();
