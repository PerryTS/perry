# Dev-facing targets. Perry is plain cargo (no mise), so this is intentionally
# minimal — add targets here only when a plain `cargo <verb>` isn't enough.

# Apply clippy's machine-applicable autofixes across the workspace. Prefers
# cargo-fixit (crate-ci/cargo-fixit, pinned 0.1.13 by the fleet's
# scripts/fleet/_shared/rust-tool-pins.mts) — the drop-in, ~20x-faster
# replacement for `cargo clippy --fix` on repeated runs, because it skips the
# full re-check compile between fix rounds. Falls back to `cargo clippy --fix`
# when cargo-fixit isn't installed; install it with
# `cargo install cargo-fixit@0.1.13 --locked`. Mirrors the fleet's
# `scripts/fleet/lint-rust.mts --fix`. Run on a dirty tree is intended
# (--allow-dirty --allow-staged); review the diff before committing. The CI
# clippy gate is unchanged.
.PHONY: fix
fix:
	@if cargo fixit --version >/dev/null 2>&1; then \
		echo "fix: cargo fixit --clippy (cargo-fixit@0.1.13)"; \
		cargo fixit --clippy --workspace --all-targets --allow-dirty --allow-staged; \
	else \
		echo "fix: cargo clippy --fix (cargo-fixit not installed; cargo install cargo-fixit@0.1.13 --locked for ~20x faster repeated runs)"; \
		cargo clippy --fix --workspace --all-targets --allow-dirty --allow-staged -- -D warnings; \
	fi
