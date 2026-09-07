# Capturing a Linux server hang or memory slope

Issue #9942 lost the spinning process before a user-space stack was captured.
`scripts/capture_linux_incident.py` runs outside the affected process, so it
works even when the JS event loop cannot run timers or signal callbacks.
It reads `/proc`; it does not restart or signal the server.

Before restarting a wedged service, capture its current PID:

```sh
python3 scripts/capture_linux_incident.py 3296296 \
  --output /tmp/perry-incident-20260907 --perf-seconds 10
```

The output directory must not exist. It is created with owner-only access.
Use the service account (or an account permitted to inspect it). Missing
`smaps_rollup`, kernel stacks, or perf permissions are recorded explicitly;
available evidence is retained. `perf` is optional and is never installed by
the script. An unsuccessful requested perf recording makes the command fail
while preserving the `/proc` samples.

For the steady growth reported in #9942, collect a healthy baseline and a later
sample under the same idle scheduler workload:

```sh
python3 scripts/capture_linux_incident.py 3296296 \
  --output /tmp/perry-growth-20260907 --samples 61 --interval 1
```

`summary.json` contains per-interval RSS growth and per-thread CPU percentages
(100% means one busy core), derived using the host clock tick and page sizes.
Thread IDs are compared with their start time so a recycled ID cannot produce
a false CPU delta. A replaced process ends the capture instead of mixing two
server lifetimes. Interrupting collection preserves the completed samples.

Each sample retains `status`, `smaps_rollup`, `maps`, `io`, `limits`, and per-thread
`stat`, `wchan`, `stack`, and `schedstat`. `/proc/.../stack` is a **kernel** stack;
it cannot identify a spinning Rust/JS function. For that, inspect the optional
user-space profile alongside the exact deployed executable and matching symbols:

```sh
perf report --stdio -i /tmp/perry-incident-20260907/perf.data
```

The default perf call chain uses frame pointers. Record whether the deployed
runtime and executable contain frame pointers/debug symbols if its stack is
incomplete. Keep the build commit, workload duration, and the last application
error beside the capture. Maps and profiles contain executable paths and
addresses; review the artifact before attaching it publicly. The collector does
not read the process environment, command line, request bodies, or application
heap.

This is evidence collection, not a fix for #9942. RSS growth alone does not
distinguish retained live objects, allocator retention, or an off-heap leak.
The profile and memory breakdown are intended to choose the next reproducer.

Collector tests (also runnable off Linux using synthetic `/proc` fixtures):

```sh
python3 -m unittest discover -s scripts -p test_capture_linux_incident.py
```
