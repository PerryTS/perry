**Tooling: run every `lint` gate locally in one command.**

`scripts/run_lint_gates.sh` derives the gate list from `.github/workflows/test.yml`
at run time — it cannot drift from what CI does — and runs all ~47 of them,
reporting each pass/fail and exiting non-zero if any failed.

It exists because the `lint` job invokes far more gates than anyone runs by hand.
On 2026-08-17 five separate gates went red on `main` in one day
(`gc_runtime_root_holders` after #8270, `-D warnings` after #8294,
`api-docs-drift` after #8279, and `raw_handle_debt` after both #8269 and #8299),
each because the reviewer ran the handful that looked topically relevant to the
diff. Every one was found only when a later PR tripped over it. A gate you did
not run is indistinguishable from a gate that passed.
