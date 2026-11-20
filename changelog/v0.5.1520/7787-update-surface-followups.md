### Fixed

Five defects in the update surface, all found in review of
[#7749](https://github.com/PerryTS/perry/pull/7749) after it had merged.

**The config warning escaped the rules that were meant to silence it.** The
"unrecognized `[update] mode`" line was printed inside `UpdatePolicy::resolve`,
before the precedence rules it sits behind had been applied — so it reached
stderr during `--format json`, in CI, with a piped stderr, and under `--quiet`.
Those rules exist to keep exactly those runs silent, and the one line whose job
was to report a config problem was the one line ignoring them. It is now held on
the policy and emitted at the single point where the run is known to be speaking
at all.

**The notify interval throttled on time alone, so it swallowed the next
release.** The documented contract is that the interval throttles repeats of
*the same* update. Keyed only on a timestamp, it also suppressed a **different**
version that arrived inside the window — so somebody setting a week-long
interval to stop being nagged about one release would also have been denied the
release that fixed it. The cache now records which version it announced, and a
different version is announced regardless of the interval.

**The interval comparison was signed.** `Duration::as_secs() as i64` goes
negative for a large enough configured value, and a negative interval reads as
already-elapsed — so an absurd value would have notified on *every* run instead
of suppressing. The comparison is unsigned.

**Two `perry` processes could corrupt the cache.** Every write used one shared
`*.json.tmp`, so two writers each wrote it and each renamed it: the loser's
rename landed a file the winner was still writing into. Each write now builds
its own temporary name.

**A refresh could erase a notice recorded while its request was in flight.**
`fetch_latest_version` read the notice state *before* issuing its request and
wrote it back afterwards, overwriting anything recorded in between — telling the
user about the same release twice. The read-modify-write pairs are now
serialized by a lock file, and the refresh re-reads inside that lock immediately
before replacing.

<details>
<summary><b>Tests</b></summary>

Two new contract tests, both sabotage-verified — reverting either fix turns its
test red:

- a different version is announced regardless of the interval, and never having
  announced anything counts as "not this version";
- an enormous interval still suppresses rather than wrapping into notifying.

The existing interval tests are unchanged in intent: they now go through a
helper that holds the announced version constant, so they still exercise only
the interval arithmetic.

`cargo test -p perry`: 904 passed, 0 failed.
</details>
