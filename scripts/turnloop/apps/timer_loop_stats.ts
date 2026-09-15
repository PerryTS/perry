// turnloop P3 — the `PERRY_LOOP_STATS=1` subject for a timer workload.
//
// Three shapes in one run, because each stresses a different part of the park:
//
//   1. a QUIET timer: one `setTimeout` far enough out that the loop has nothing
//      else to do. It must cost ONE wait, not a spin — `turns` and `os_waits`
//      must not scale with the delay.
//   2. a SUB-MILLISECOND remainder: a deadline less than a millisecond away,
//      which is the shape P0's `as_millis()` truncation turned into a spin.
//   3. CHURN: many short timers and an interval, so `timer_arms` and
//      `timer_expiries` are both large and the arming is demonstrably live.
//
// Run as:
//   PERRY_LOOP_STATS=1 ./timer_loop_stats
// and read the `[perry-loop]` line on stderr.

function sleep(ms: number): Promise<void> {
  return new Promise<void>((resolve) => setTimeout(resolve, ms));
}

async function main(): Promise<void> {
  // 1 — quiet parks.
  const quietStart = Date.now();
  for (let i = 0; i < 5; i++) {
    await sleep(40);
  }
  console.log("quiet parks done in >= 200ms:", Date.now() - quietStart >= 200);

  // 2 — a sub-millisecond remainder: burn most of a 2 ms timer, then await it.
  let subMs = 0;
  for (let i = 0; i < 20; i++) {
    const start = performance.now();
    const remainder = sleep(2);
    while (performance.now() - start < 1.6) {
      /* leave less than a millisecond for the park */
    }
    await remainder;
    subMs++;
  }
  console.log("sub-millisecond remainders:", subMs);

  // 3 — churn: 2000 short timeouts, 200 immediates and an interval.
  let fired = 0;
  await new Promise<void>((resolve) => {
    for (let i = 0; i < 2000; i++) {
      setTimeout(() => {
        fired++;
        if (fired === 2000) resolve();
      }, 1 + (i % 7));
    }
  });
  console.log("timeouts fired:", fired);

  let immediates = 0;
  await new Promise<void>((resolve) => {
    const step = () => {
      immediates++;
      if (immediates === 200) resolve();
      else setImmediate(step);
    };
    setImmediate(step);
  });
  console.log("immediates fired:", immediates);

  let ticks = 0;
  await new Promise<void>((resolve) => {
    const handle = setInterval(() => {
      ticks++;
      if (ticks === 50) {
        clearInterval(handle);
        resolve();
      }
    }, 1);
  });
  console.log("interval ticks:", ticks);

  // A cancelled timer must not extend the run.
  const cancelled = setTimeout(() => console.log("CANCELLED TIMER FIRED"), 60_000);
  clearTimeout(cancelled);
  console.log("done");
}

main();
