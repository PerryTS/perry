// Gap test: the npm `cron` package's CronJob class (distinct from
// node-cron's schedule() factory). `new CronJob(expr, fn)` must NOT
// auto-start; the 4-arg form with start=true must; start()/stop() must
// dispatch. Tick counts are asserted as booleans and only the first two
// manual ticks print, so output is deterministic despite the timing.

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

  // A never-started job must not fire (would print below and break the diff).
  const never = new CronJob("* * * * * *", () => {
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
  await new Promise((resolve) => setTimeout(resolve, 3200));
  job.stop();
  auto.stop();

  console.log("manual ticked at least twice:", ticks >= 2);
  console.log("auto ticked at least twice:", autoTicks >= 2);
  console.log("never-started stayed quiet:", true);
  console.log("done");
}

main();
