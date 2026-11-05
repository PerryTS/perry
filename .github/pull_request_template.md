<!--
Thanks for contributing to Perry! A few things to know before you submit:

1. Please do NOT bump `[workspace.package] version` in Cargo.toml.
2. Please do NOT edit CHANGELOG.md.

The maintainer folds all of that metadata in at merge time — Perry
releases frequently, and version bumps conflict if they live on the
PR branch. See CONTRIBUTING.md for the reasoning.
-->

## Summary

<!-- One or two sentences on what this PR changes and why. -->

## Changes

<!--
- Bullet list of concrete changes, one per logical item.
- File paths optional but appreciated for non-trivial PRs.
-->

## Related issue

<!-- Fixes #123 / Closes #123 / Refs #123 — or "n/a" if standalone. -->

## Test plan

<!--
How did you verify this works? A paste of the commands you ran is perfect.
-->

- [ ] `cargo build --release` clean
- [ ] `cargo test --workspace --exclude perry-ui-ios --exclude perry-ui-tvos --exclude perry-ui-watchos --exclude perry-ui-gtk4 --exclude perry-ui-android --exclude perry-ui-windows` passes
- [ ] (if user-facing) Added or updated a test under `test-files/` or a `#[test]` in the affected crate
- [ ] (if CLI / stdlib / runtime API changed) Updated `docs/src/`
- [ ] (if touching a platform UI backend) Built `-p perry-ui-<backend>` locally on that platform

## Screenshots / output

<!-- Optional. If the change is visible (UI, CLI output, error messages), paste before/after. -->

## Checklist

- [ ] I have NOT bumped the workspace version or edited CHANGELOG.md (maintainer handles these at merge)
- [ ] My commits follow the loose `feat:` / `fix:` / `docs:` / `chore:` prefix convention used in the log
- [ ] I've read [CONTRIBUTING.md](../CONTRIBUTING.md) and agree to the [Code of Conduct](../CODE_OF_CONDUCT.md)
