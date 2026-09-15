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
Diagnostic only — with `PERRY_LOOP_STATS` unset every hook is one relaxed atomic
load, with no allocation and no lock on any wait path.

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
