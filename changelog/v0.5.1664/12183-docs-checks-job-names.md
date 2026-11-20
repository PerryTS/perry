### Fix documentation validation and lowercase Actions job names

- Add the WeakMap owned-storage design and validation pages to the mdBook
  contents, fixing Repository run #128's documentation validation failure.
- Standardize job labels to lowercase and give nested jobs their stable suite
  ID prefix, restoring monitor and simulator release-gate matching. Preserve
  workflow category names, execution settings, dependencies, and parallelism.
- Enforce lowercase labels, suite prefixes, and valid freshness selectors in
  the existing Actions topology check. Failure monitoring now reads the inline
  suite result so optional skipped children do not conceal a completed verdict.

Validation: documentation checks passed for all 151 chapters against deployed
release v0.5.1520. Actions topology and failure-monitor configuration checks
passed. Parsed execution configuration was identical for all six workflow
files after excluding job display names. Full hosted CI runs on the PR.
