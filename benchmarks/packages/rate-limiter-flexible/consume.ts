// rate-limiter-flexible: RateLimiterMemory.consume over 200 keys with a
// 1-hour window (no resets during a run), counting accepts and rejects.
// msBeforeNext is time-dependent, so it is never printed.
import { RateLimiterMemory } from "rate-limiter-flexible";
import { iters, header } from "../_lib/bench.ts";

const it = iters(20000, 1000);
header("rate-limiter-flexible/consume", "rate-limiter-flexible", it);
// Loop bounds as locals: re-reading `it.n` per iteration would add harness
// cost to the measurement (Perry: a by-name property read, ~1k instructions).
const N = it.n, WARM = it.warm;

async function main(): Promise<void> {
  const run = async (n: number): Promise<string> => {
    const rl = new RateLimiterMemory({ points: 50, duration: 3600 });
    let ok = 0, rejected = 0, remaining = 0;
    for (let i = 0; i < n; i++) {
      try {
        const r = await rl.consume("user:" + (i % 200), 1 + (i % 2));
        ok++; remaining += r.remainingPoints;
      } catch (e: any) {
        if (e instanceof Error) throw e;
        rejected++;
      }
    }
    return "ok " + ok + " rejected " + rejected + " remaining " + remaining;
  };
  await run(WARM);
  console.log(await run(N));
}
main().catch((e) => { console.log("ERROR " + (e && e.message)); process.exitCode = 1; });
