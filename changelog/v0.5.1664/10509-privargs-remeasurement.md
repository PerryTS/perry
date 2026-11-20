Add reproducible instruction-count micros for private fields and methods,
escaping and unused arguments objects, and typed-array parameter reads with
and without unrelated ArrayBuffer aliasing. Each construct has a branch-free
kernel; Node counters run only after warmup. The harness also measures the
shared real-program drivers and four class micros with separate build arms.
