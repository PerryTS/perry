**Fix a race in `queued_writes_report_backpressure_and_drain_in_order`, flaky since it was written.**

Two lanes reported it independently — 8 pass / 4 fail over 12 runs, and 2 of 6 —
and both correctly declined to chase it as their own regression. It is a defect
in the test, not in the code under test:

```rust
super::tcp_connect(client, SUBSYSTEM, local, true).expect("connect");
assert!(pump_until(|e| e.iter().any(|e| e.kind == NET_CONNECT)));
let conn = accepted_id(server).expect("connection id");
```

It pumps until the **client's** `NET_CONNECT` completion arrives, then
immediately requires the **server's** `NET_ACCEPT` event. Those are two
independent completions and nothing orders them. On a loaded machine the accept
lands in a later turn, `accepted_id` returns `None`, and the `expect` panics at
`tests.rs:321:36` — the line every report named.

The fix is the pattern the UDS test three hundred lines below already uses:
wait for both ends to establish. `pump_until` carries a bounded 5-second budget,
so waiting on the stronger condition cannot hang.

A/B on one machine state, 12 runs each: **4 of 12 fail before, 0 of 12 after.**
That rules out a timing perturbation — the failure rate matches what both lanes
measured on the pristine tree.

Worth noting for the next such report: a test that waits for the wrong event
fails only under load, which makes it look like flakiness in the subsystem
rather than a missing precondition in the test. The discriminating question is
whether the thing the next line needs is the thing the wait actually waited for.
