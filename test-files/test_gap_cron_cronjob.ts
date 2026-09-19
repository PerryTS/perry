// Gap test: the npm `cron` package's CronJob class (distinct from
// node-cron's schedule() factory). `new CronJob(expr, fn)` must NOT
// auto-start; the 4-arg form with start=true must; start()/stop() must
// dispatch. Only the first two manual ticks print, so the output does not
// depend on how many ticks land.
//
// #10581: the wait below is a BARRIER, not a deadline. It used to race a fixed
// 10-second wall clock against a one-per-second schedule and print `false` when
// the clock won -- classified as a `parity_fail`, i.e. read as a miscompile, on
// a loaded runner. (`PERRY_RUN_TIMEOUT` is itself 10s, so that deadline could
// only ever fire in a photo finish with the harness's own kill.) With no
// deadline the printed text is a function of CronJob's behaviour alone: either
// the ticks arrive and the output below is produced, or nothing dispatches and
// the harness kills the run -- which it classifies as a CRASH/timeout,
// distinctly from a parity mismatch. Deliberately no fallback bound: any bound
// that prints, throws or exits differently on expiry reintroduces exactly this
// defect at a different threshold. The fixture is not permitted to decide it
// has waited long enough; two ticks of `* * * * * *` take ~2s.

import { CronJob } from "cron";

async function main() {
  let ticks = 0;
  const job = new CronJob("* * * * * *", () => {
    ticks++;
    if (ticks <= 2) {
      console.log("tick", ticks);
    }
  });
  console.log("constructed, ticks now:", ticks);

  // A never-started job must not fire (the log below would break the diff, and
  // the counter is asserted after the barrier, i.e. after >= 2 cron seconds
  // have demonstrably elapsed).
  let neverTicks = 0;
  const never = new CronJob("* * * * * *", () => {
    neverTicks++;
    console.log("SHOULD-NOT-RUN");
  });

  // 4-arg form: onComplete null, start=true — begins firing immediately.
  let autoTicks = 0;
  const auto = new CronJob(
    "* * * * * *",
    () => {
      autoTicks++;
    },
    null,
    true
  );

  job.start();
  while (ticks < 2 || autoTicks < 2) {
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  job.stop();
  auto.stop();

  console.log("manual ticked at least twice:", ticks >= 2);
  console.log("auto ticked at least twice:", autoTicks >= 2);
  console.log("never-started stayed quiet:", neverTicks === 0);
  console.log("done");
}

main();
