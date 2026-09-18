`PERRY_LOOP_STATS=1` now measures the waits themselves, not just how many there
were. Instruction counts and RSS can stay flat while the waits between Perry and
tokio decide a server's latency and CPU, so the exit report adds, for the primary
agent: per wait kind — a turnloop turn, a transitional tokio tick, a condvar park
— the count, total and maximum time parked; the count, total and maximum time of
stdlib fast drives that actually drove tokio; a wake-latency histogram
(`<50µs`, `<200µs`, `<1ms`, `<5ms`, `≥5ms`, plus the maximum) measured from a
producer's notify — cross-thread or an in-thread native completion — to the
parked wait returning; and the number of zero-budget returns and #1114
spin-throttle sleeps. It prints as one `[perry-loop-waits] arm=… key=value` line
at the process-exit funnel.

Every counter is recorded identically in **both** A/B arms (`tokio-wait-driver`
on and off), so the two arms can be compared like with like: the same tokio tick
is measured in both, and the `arm=` field says which build produced the line.
Both wake producers stamp the clock — `js_notify_main_thread` and, separately,
`js_native_work_submitted`, which wakes a parked turn directly and is the only
way a cross-thread native submission reaches one. A stamp that belongs to an
earlier wait is rejected rather than attributed to the next one, so the
histogram can under-report a wake but never invent or inflate one. Diagnostic
only — with `PERRY_LOOP_STATS` unset every hook is one relaxed atomic load, with
no allocation and no lock on any wait path.

`scripts/turnloop/server_ab.py` is the server A/B harness for that comparison
(Linux; `--dry-run` works anywhere). It builds both arms from one commit into
separate target dirs, records each archive's mtime, size and SHA-256, compiles
the same `node:http` app with each, and then interleaves the arms over N rounds
of load scenarios (`oha`, else `wrk`) at each requested concurrency plus idle
keep-alive capacity tests, collecting throughput, p50/p99/p999, CPU user/sys,
wall, voluntary and involuntary context switches, syscalls/s (`perf stat -e
raw_syscalls:sys_enter`, else `strace -c -f`), peak RSS, bytes per idle
connection, binary size and the wait metrics above. A sample whose arm marker or
`arm=` field does not match the arm it was supposed to measure is rejected rather
than averaged in. Output is one markdown table plus JSON.

Two findings fell out of building the harness. `server.keepAliveTimeout = 0` on
a `node:http` server means "never time out" in Node but "no keep-alive" in
Perry, which answers `Connection: close` — unrelated to turnloop (both arms
behave identically) and not fixed here, but it is why the harness app sets a
large timeout instead. And a P0 server makes **zero** turnloop turns: its accept
loop keeps a tokio task alive for the life of the process, so every park goes to
the transitional tick. That is the documented P0 design rather than a
regression, and the new line is what will show P1 changing it.

The harness also reports hardware counters under load, and keeps them apart from
the deterministic ones. `perf stat` over each measured window gives RETIRED
instructions, cycles, IPC, task-clock, context switches, CPU migrations, page
faults and the syscall tracepoint — each also normalised per request, so a
throughput win cannot hide a per-request regression. A `callgrind` subcommand
reports Valgrind `Ir`, instructions EXECUTED, in its own section with its own
caveat (deterministic and load-independent by construction; microbenchmarks
only, because Valgrind's 50-100x cost turns a server run into an artificial wait
pattern). When `perf` cannot run, its rows are still rendered with the reason
rather than dropped. The harness prints the host it ran on and marks a sample's
throughput and latency ADVISORY on a shared build box or above `--max-loadavg`,
while leaving the per-process counters authoritative — so counters can come from
a shared Linux box and timing from a quiet one, with the report saying which is
which.
