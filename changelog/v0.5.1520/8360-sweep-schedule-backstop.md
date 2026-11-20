### CI: two-hourly cron backstop for the main sweep (`main-gate`)

Measured 2026-08-16..18: **no push-triggered sweep ever reached a runner.**
The coalescing group keeps one running + one pending run and replaces the
pending with each newer merge — and merges landed faster than the
pending→running transition, even with an idle queue, so `main-gate` had never
once executed (and main-line rust-cache/sccache saves only happened on
nightlies). A `47 */2 * * *` cron now fires the sweep reliably;
`scripts/ci_plan.py` maps the nightly cron to `full` and any other cron to
`sweep` (self-tested both ways). The push arm stays for quiet periods.

Note: the sweep's full `cargo-test` legitimately fails on #8222 today, so
`main-gate` reports red until that lands — deliberately not registered in
`gate_freshness.json` until then (a freshness alarm on a known-red gate is
noise).
