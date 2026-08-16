### Changed

- **The public performance baseline now fingerprints a small declarative
  measurement protocol instead of benchmark-runner plumbing (#7282).** Pinned
  toolchains, quiet-host thresholds, samples, warmups, selected workloads,
  kernels, fixture generators, and the correctness oracle remain hard-gated;
  logging, cleanup, and output-format changes no longer demand a two-hour
  baseline regeneration.
