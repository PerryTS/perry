// rate-limiter-flexible: get/penalty/reward/delete bookkeeping on RateLimiterMemory.
import { RateLimiterMemory } from "rate-limiter-flexible";
import { iters, header } from "../_lib/bench.ts";

const it = iters(20000, 1000);
header("rate-limiter-flexible/get_penalty", "rate-limiter-flexible", it);
// Loop bounds as locals: re-reading `it.n` per iteration would add harness
// cost to the measurement (Perry: a by-name property read, ~1k instructions).
const N = it.n, WARM = it.warm;

async function main(): Promise<void> {
  const run = async (n: number): Promise<string> => {
    const rl = new RateLimiterMemory({ points: 1000, duration: 3600 });
    let seen = 0, consumed = 0, dels = 0;
    for (let i = 0; i < n; i++) {
      const key = "ip:" + (i % 300);
      switch (i & 3) {
        case 0: { const r = await rl.get(key); if (r) { seen++; consumed += r.consumedPoints; } break; }
        case 1: await rl.penalty(key, 3); break;
        case 2: await rl.reward(key, 1); break;
        default: if (i % 97 === 3) { if (await rl.delete(key)) dels++; } else await rl.penalty(key, 1);
      }
    }
    return "seen " + seen + " consumed " + consumed + " dels " + dels;
  };
  await run(WARM);
  console.log(await run(N));
}
main().catch((e) => { console.log("ERROR " + (e && e.message)); process.exitCode = 1; });
