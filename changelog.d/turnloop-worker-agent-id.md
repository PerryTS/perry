`worker_threads` Workers now claim an agent id.

`perry-stdlib`'s Worker spawned its OS thread with a bare `std::thread::spawn`
and never called `agent::enter_worker_agent()`, so `current_agent()` returned
`PRIMARY_AGENT` on a thread that owns neither the primary heap nor its loop.
That was invisible until the turnloop migration gave `PRIMARY_AGENT` a meaning
beyond queue ownership: `agent_loop::net_available()` is
`current_agent() == PRIMARY_AGENT`, so a Worker's `fetch()` passed the submit
guard and was then refused by `ensure_loop_with` a moment later — failing after
acceptance instead of taking the fallback path. `fetch()` inside a Worker
returned 200 on `main` and `error: fetch failed` on the migration branch.

The Worker now enters an agent before it can allocate or enqueue anything and
retires it at thread exit, which is what `perry/thread`'s `spawn`,
`parallelMap` and `parallelFilter` have always done (#6185).
