- **`await-tests` can accept an already-validated ancestor's gate when the only
  difference is release plumbing.** `test.yml` does not build the glibc image,
  read `changelog.d/`, or run `release-packages.yml` — so a candidate that
  differs from a green ancestor *only* in those files is already covered by that
  ancestor's `full-suite-gate`. Re-running a ~5 h tier to re-prove untouched code
  is pure latency, and this campaign paid it four separate times over one
  Dockerfile.

  Fail-closed by construction:

  - the file list comes from **GitHub's compare API**, computed from the commits
    themselves — never from a dispatch input;
  - **any** path outside the allowlist keeps the exact-SHA requirement;
  - an **empty** diff is refused, since it should be impossible here and would
    mean the comparison did not do what we think;
  - **`.github/workflows/test.yml` is deliberately not allowlisted** — changing
    the tier's own definition must re-run the tier.

  Allowlist: `changelog.d/**`, `scripts/linux-*.Dockerfile`,
  `.github/workflows/release-packages.yml`.

  Verified against the live repo before landing: on pin `3216910e19` the resolver
  selects ancestor `49132f00cd` (whose gate is green) because the diff is exactly
  `changelog.d/9665-…md` + `scripts/linux-glibc-2.31.Dockerfile`. Sabotage-checked
  in the other direction too — a single `crates/**` file, `test.yml`, `Cargo.toml`,
  a path-traversal string, or an empty diff each force the exact-SHA gate.
