<!--
Thanks for contributing to Perry! Before you submit:

1. Do NOT bump `[workspace.package] version` in Cargo.toml.
2. Do NOT edit CHANGELOG.md; it is frozen. A change under `crates/` adds
   `changelog.d/<PR-number>-<slug>.md` instead (see changelog.d/README.md).

One logical change per PR. See CONTRIBUTING.md for the reasoning.
-->

## Summary

<!-- One or two sentences on what this PR changes and why. -->

## Related issue

<!-- Fixes #123 / Closes #123 / Refs #123, or "n/a" if standalone. -->

## Type of change

- [ ] Bug fix
- [ ] New feature
- [ ] Performance (speed, compile time or memory)
- [ ] Binary or code size
- [ ] Refactor, CI, docs or cleanup

## Platforms

<!-- Tick what the change affects, and say where you actually ran it. -->

- [ ] All platforms
- [ ] macOS
- [ ] Linux
- [ ] Windows
- [ ] iOS
- [ ] Android
- [ ] WebAssembly / web
- [ ] Other: <!-- tvOS, watchOS, visionOS, Wear OS, HarmonyOS… -->

Tested on: <!-- e.g. macOS 15.1 arm64; iPhone 15 simulator (iOS 18.1); Ubuntu 24.04 x86_64 -->

## Changes

<!--
- One bullet per logical item.
- For a user-visible change, list the paths it touches (CLI, stdlib, runtime API, docs/src/).
-->

## Test plan

<!-- The commands you actually ran, with their result. A paste is perfect. -->

```bash

```

- [ ] Added or updated a test in the layer that owns it: a unit test beside the code, `crates/*/tests/`, or a parity case under `tests/parity/` (see tests/README.md)
- [ ] `scripts/run_lint_gates.sh` passes
- [ ] (CLI, stdlib or runtime API changed) Updated `docs/src/`
- [ ] (platform UI backend touched) Built `-p perry-ui-<backend>` on that platform

## Screenshots / output

<!-- Optional. For visible changes (UI, CLI output, error messages), paste before/after. -->

## Checklist

- [ ] I have NOT bumped the workspace version or edited CHANGELOG.md
- [ ] A change under `crates/` adds `changelog.d/<PR-number>-<slug>.md` (or the PR is labelled `skip-changelog`)
- [ ] My commits follow the `feat:` / `fix:` / `docs:` / `chore:` prefix convention
- [ ] I've read [CONTRIBUTING.md](../CONTRIBUTING.md) and agree to the [Code of Conduct](../CODE_OF_CONDUCT.md)
