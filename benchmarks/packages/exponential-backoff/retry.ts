// exponential-backoff: backOff() over an operation that fails 3 times then
// succeeds (startingDelay 0, no jitter — measures the retry machinery).
import { backOff } from "exponential-backoff";
import { iters, header } from "../_lib/bench.ts";

const it = iters(2000, 100);
header("exponential-backoff/retry", "exponential-backoff", it);
// Loop bounds as locals: re-reading `it.n` per iteration would add harness
// cost to the measurement (Perry: a by-name property read, ~1k instructions).
const N = it.n, WARM = it.warm;

async function main(): Promise<void> {
  let attempts = 0, retries = 0, total = 0;
  const once = async (i: number): Promise<void> => {
    let a = 0;
    const r = await backOff(async () => {
      a++; attempts++;
      if (a < 4) throw new Error("transient " + a);
      return i * 2;
    }, { numOfAttempts: 5, startingDelay: 0, timeMultiple: 1, jitter: "none",
         retry: () => { retries++; return true; } });
    total += r;
  };
  for (let i = 0; i < WARM; i++) await once(i);
  attempts = 0; retries = 0; total = 0;
  for (let i = 0; i < N; i++) await once(i);
  console.log("attempts " + attempts + " retries " + retries + " total " + total);
}
main().catch((e) => { console.log("ERROR " + (e && e.message)); process.exitCode = 1; });
