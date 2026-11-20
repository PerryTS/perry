// Gap test: rate-limiter-flexible's RateLimiterMemory must construct a
// real limiter (not `{}`) and dispatch consume/get/delete. Consumption
// counts down within the fixed window; exceeding the quota rejects with
// a RateLimiterRes-shaped value. msBeforeNext is time-dependent, so it
// is only asserted as a boolean.

import { RateLimiterMemory } from "rate-limiter-flexible";

async function main() {
  const limiter = new RateLimiterMemory({ points: 3, duration: 60 });

  const r1 = await limiter.consume("alice");
  console.log("r1", r1.remainingPoints, r1.consumedPoints, r1.isFirstInDuration);
  const r2 = await limiter.consume("alice");
  console.log("r2", r2.remainingPoints, r2.consumedPoints, r2.isFirstInDuration);
  const r3 = await limiter.consume("alice", 1);
  console.log("r3", r3.remainingPoints, r3.consumedPoints);

  // Quota exhausted: consume must reject with a res-shaped value.
  try {
    await limiter.consume("alice");
    console.log("unexpected: not limited");
  } catch (rej: any) {
    console.log("limited", rej.remainingPoints, "msBeforeNext positive:", rej.msBeforeNext > 0);
  }

  // Independent key, multi-point consume.
  const bob = await limiter.consume("bob", 2);
  console.log("bob", bob.remainingPoints, bob.consumedPoints, bob.isFirstInDuration);

  // get() reads without consuming; unknown key resolves null.
  const got = await limiter.get("alice");
  console.log("get alice", got === null ? "null" : got.remainingPoints);
  const gotNone = await limiter.get("carol");
  console.log("get carol", gotNone === null ? "null" : "not-null");

  // delete() drops the window; the key becomes fresh again.
  const del = await limiter.delete("alice");
  console.log("deleted", del);
  const after = await limiter.get("alice");
  console.log("after delete", after === null ? "null" : "not-null");

  const fresh = await limiter.consume("alice");
  console.log("fresh", fresh.remainingPoints, fresh.consumedPoints, fresh.isFirstInDuration);
}

main();
