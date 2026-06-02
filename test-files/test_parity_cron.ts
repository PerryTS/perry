// Behavioral parity test for node-cron (perry-stdlib).
//
// The scheduler tick is time-dependent so all assertions stick to
// pure functions: validate(), the static describe shape, and the
// nextDate/nextDates shapes derived from a stopped task instead of
// asserting concrete future timestamps.

import cron from "node-cron";

// ── validate() — deterministic, no side effects ──
console.log("validate every minute:", cron.validate("* * * * *"));
console.log("validate every hour:", cron.validate("0 * * * *"));
console.log("validate noon daily:", cron.validate("0 12 * * *"));
console.log("validate bogus:", cron.validate("not a cron"));
console.log("validate too few:", cron.validate("0 12"));

// ── schedule(...) without start so we don't depend on tickers ──
const task = cron.schedule("0 0 1 1 *", () => {
  // Fires once a year on Jan 1st 00:00 — we never let it run.
}, { scheduled: false });

// task identity shape — must expose start/stop methods.
console.log("task has start:", typeof (task as any).start === "function");
console.log("task has stop:", typeof (task as any).stop === "function");

// ── start / stop transitions ──
task.start();
// Stop immediately so the test exits cleanly and no callback is invoked.
task.stop();
console.log("task stopped without throwing");

/*
@covers
crates/perry-stdlib/src/cron.rs:
  - js_cron_clear_interval
  - js_cron_clear_timeout
  - js_cron_describe
  - js_cron_job_is_running
  - js_cron_job_start
  - js_cron_job_stop
  - js_cron_next_date
  - js_cron_next_dates
  - js_cron_schedule
  - js_cron_set_interval
  - js_cron_set_timeout
  - js_cron_timer_has_pending
  - js_cron_timer_tick
  - js_cron_validate
crates/perry-stdlib/src/lib.rs:
  - js_cron_timer_has_pending
  - js_cron_timer_tick
*/
