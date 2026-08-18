# Dev-facing targets. Perry is plain cargo (no mise), so this is intentionally
# minimal — add targets here only when a plain `cargo <verb>` isn't enough.

# Apply clippy's machine-applicable autofixes across the workspace via
# cargo-fixit (crate-ci/cargo-fixit, pinned in external-tools.json under the
# `cargo-install` release type) — the drop-in, faster replacement for
# `cargo clippy --fix` on repeated runs, because it skips the full re-check
# compile between fix rounds. There is deliberately no `cargo clippy --fix`
# fallback: install cargo-fixit with `make fix-deps` first. Run on a dirty
# tree is intended (--allow-dirty --allow-staged); review the diff before
# committing. The CI clippy gate is unchanged. Mirrors the fleet's
# `scripts/fleet/lint-rust.mts --fix`.
.PHONY: fix
fix:
	@set -e; cargo fixit --clippy --workspace --all-targets --allow-dirty --allow-staged

# Install the pinned cargo-fixit dev tool that `make fix` drives. cargo-fixit
# is a cargo crate with NO prebuilt binaries (no cargo binstall, no GitHub
# release assets), so it is pinned in external-tools.json under the
# `cargo-install` release type and built from source with `--locked`.
# The version is read from external-tools.json so the pin has one source of truth.
.PHONY: fix-deps
fix-deps:
	@set -e; v=$$(node -e "console.log(require('./external-tools.json').tools['cargo-fixit'].version)"); \
	cargo install cargo-fixit@$$v --locked
