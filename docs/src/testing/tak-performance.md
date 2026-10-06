# Perry CLI performance reports

The scheduled and release-triggered `tak Performance Report` workflow measures
four local Perry commands: startup/version, help output, a full compile of the
JSON parsing app-pattern kernel, and the compiler's `check` path on a second
app-pattern kernel. The workloads are declared in [`tak.toml`](../../../tak.toml).
They read only checked-in sources and do not access the network.

The workflow runs on Ubuntu 24.04 with tak 0.0.14 and the pinned Valgrind
package from `external-tools.json`. Cargo sources, Perry build outputs, and the
pinned tak binary are cached in CI. The full measurement-series class, including
these tool versions and the Rust toolchain, is recorded in `tak.toml`. It attaches the measurement
summary to the Actions run and uploads JSON plus text output as an artifact. A
separate trusted job publishes the exact-SHA measurement into Git notes under
`refs/notes/tak`; the build and measurement job itself has no write permission.
The run also renders recent measurements and `tak detect`'s instruction-count
steps, which name the commit where each step appeared. This starts a history;
the first measurement on a runner has nothing to compare against.

The workflow is report-only and does not gate pull requests. Wall-clock samples
are informational. tak's instruction counts may be considered for a gate after
reviewing a stable history on the same runner class. The class is pinned in
`tak.toml` so a toolchain change starts a separate comparison series.

To inspect the configured workloads locally, install tak and run:

```sh
cargo install tak-cli@0.0.14 --locked
tak run --dry-run
```

Run and record measurements with `tak run --no-progress --record`. See history
with `tak log --limit 10`, or identify instruction-count steps with
`tak detect --no-gate`.
Instruction counts require Linux and Valgrind; Apple Silicon macOS can report
timings only.

This report complements the [app-pattern benchmarks](../../../benchmarks/app-patterns/README.md),
which compare generated-program runtime against Bun and Node with checksummed
outputs. It is separate from the published [Node/Bun performance baseline](../../../benchmarks/results/public-node-bun-v1.json)
and `benchmark.yml`'s release regression gate.
