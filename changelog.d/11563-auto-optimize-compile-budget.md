**Parity harness: an auto-optimize compile gets the toolchain budget, ext wrapper or not (#11560).**
In the full tier (auto-optimize on for the whole corpus), `run_parity_tests.sh`
gave the 900s `PERRY_TOOLCHAIN_COMPILE_TIMEOUT` only to ext-routed fixtures.
Every plain fixture that drew a new feature set rebuilt a stripped
runtime+stdlib (~200-330s on hosted runners) under the ordinary 300s budget and
was killed mid-rebuild: 42 "compile TIMEOUT after 300s" in the 09-27 full run,
none ext-routed, 36 of its 47 new parity failures. Introduced when #10757
(c78f1a88d) added the 300s compile bound. The budget is now keyed on
`auto_optimize_on`; the fast tiers (`PERRY_SKIP_BUILD=1` ⇒ no-auto) keep the
300s hang bound for plain compiles. `tests/test_parity_compile_budget.sh`
(wired into `lint`) drives the harness with a mock compiler slower than the
ordinary budget and fails on the old predicate.
