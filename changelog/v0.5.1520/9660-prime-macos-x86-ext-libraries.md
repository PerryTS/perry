- **The macOS x86_64 release leg no longer dies at GitHub's 6-hour ceiling.** In
  run 33861357826, `build (macos-15-intel, x86_64-apple-darwin)` was cancelled at
  **361 min** — GitHub's HARD 360-min hosted-runner cap, which no
  `timeout-minutes` can raise. One step accounted for it: *Build native ext
  libraries (Unix)* ran **304 min** (the three steps before it took 54 min
  combined).

  The cost is structural. That step builds **40** governed ext packages, each as
  its own cargo invocation carrying `-p perry -p perry-runtime-static
  -p perry-stdlib-static` (#7358 requires that so features unify per wrapper). A
  different package set per iteration means a different feature union, so each
  one largely rebuilds the compiler, runtime and stdlib — ~7.6 min × 40.

  `prime-macos-x86_64-cache` exists to absorb exactly this, but primed only
  `-p perry`, warming none of those 40 feature unions. It now runs the same loop
  into the same `release-x86_64-apple-darwin` shared-key cache the build leg
  reads.

  The loop is **budgeted to 250 min** of the job's 330-min cap on purpose: a job
  killed at its cap is *cancelled*, which skips rust-cache's post save step, so
  a timed-out prime warms nothing and the next attempt starts equally cold —
  the same trap that made the simctl retries in runs 33709079451 / 33718460967
  unwinnable. Stopping early lets the job end normally, which is what writes the
  cache. It reports how many packages it primed.

  Note this is margin, not a cure: `macos-14` (aarch64) finished the same work in
  **291 min**, only 69 min under the ceiling, so *both* macOS architectures run
  against the cap. If the ext build keeps growing, the durable fix is sharding
  that step across jobs so the 6-hour budget applies per shard.
