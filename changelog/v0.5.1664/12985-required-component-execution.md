Run the synchronous P2 component acceptance harnesses in the required Linux
WASI job for core PR, sweep and full CI tiers. This covers native hosting,
Perry-authored components, typed resources, moving GC and Rust/Perry composition
alongside the existing standalone command suites. The existing `pr-gate` fan-in
rejects a failed, cancelled or skipped planned job.

Retain component phase logs, JSON verdicts, component artifacts and source
revision on failure, and give the combined build a 120-minute job budget.
Docs-only PRs continue to skip execution.
