// Gap test: exponential-backoff's backOff(task, options) must honor
// numOfAttempts / startingDelay / timeMultiple / maxDelay and retry a
// promise-returning task on rejection. The native binding used to
// hardcode 3 attempts / 100ms / x2 / 10s and never retried async
// tasks at all. Elapsed-time checks are expressed as booleans (no raw
// timestamps in the output) so the test is deterministic.

import { backOff } from "exponential-backoff";

async function main() {
  // Succeeds on the 4th attempt — requires numOfAttempts > 3 to pass.
  let attempts = 0;
  const t0 = Date.now();
  const result = await backOff(
    async () => {
      attempts++;
      if (attempts < 4) {
        throw new Error("flaky-" + attempts);
      }
      return "ok:" + attempts;
    },
    { numOfAttempts: 6, startingDelay: 40, timeMultiple: 2, maxDelay: 500 }
  );
  const elapsed = Date.now() - t0;
  console.log(result);
  console.log("attempts:", attempts);
  // Delays should be ~40 + 80 + 160 = 280ms; allow generous slack both ways.
  console.log("waited at least 250ms:", elapsed >= 250);
  console.log("finished under 5s:", elapsed < 5000);

  // Exhausts numOfAttempts and rejects with the last error.
  let attempts2 = 0;
  try {
    await backOff(
      async () => {
        attempts2++;
        throw new Error("always-fails");
      },
      { numOfAttempts: 3, startingDelay: 10 }
    );
    console.log("unexpected success");
  } catch (e) {
    console.log("failed after attempts:", attempts2, "-", (e as Error).message);
  }

  // retry predicate stops the loop early.
  let attempts3 = 0;
  try {
    await backOff(
      async () => {
        attempts3++;
        throw new Error("nope");
      },
      {
        numOfAttempts: 10,
        startingDelay: 5,
        retry: (_e: unknown, attemptNumber: number) => attemptNumber < 2,
      }
    );
    console.log("unexpected success 2");
  } catch (_e) {
    console.log("predicate stopped after:", attempts3);
  }
}

main();
