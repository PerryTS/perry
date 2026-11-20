Kept `test-files/test_gap_cron_cronjob.ts`, which the binding removal deleted.

After the `cron` binding is gone, `import { CronJob } from "cron"` resolves to
the real npm package (still a declared devDependency at `^4.4.0`), so the
fixture stops testing a Rust shim and starts testing the thing the removal
claims works. That is the precedent set by merge train 231, which deliberately
retained `test_gap_dayjs_factory_arg` and `test_gap_ratelimiter_memory` for
exactly this reason: the fixture becomes the standing witness that the real
package has not diverged, and the gap gate goes red the day it does.

It is also a fixture worth not throwing away. #10581 rewrote its wait as a
**barrier rather than a deadline** after the original raced a fixed 10-second
wall clock against a one-per-second schedule and printed `false` when the clock
won — read as a miscompile on a loaded runner, when `PERRY_RUN_TIMEOUT` is
itself 10s. Its header records why no fallback bound is permitted: any bound
that prints, throws or exits differently on expiry reintroduces the same defect
at a different threshold. That reasoning is not recoverable from a fixture that
no longer exists.

It is not in `test-parity/gap_snapshot.json`, so it is expected to pass.
