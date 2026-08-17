# Dev-facing targets. Perry is plain cargo (no mise), so this is intentionally
# minimal — add targets here only when a plain `cargo <verb>` isn't enough.

# Apply clippy's machine-applicable autofixes across the workspace. Prefers
# cargo-fixit (crate-ci/cargo-fixit) — the drop-in, ~20x-faster replacement
# for `cargo clippy --fix` on repeated runs, because it skips the full
# re-check compile between fix rounds. Falls back to `cargo clippy --fix`
# when cargo-fixit isn't installed; install it with
# `cargo install cargo-fixit --locked`. Run on a dirty tree is intended
# (--allow-dirty --allow-staged); review the diff before committing. The CI
# clippy gate is unchanged.
.PHONY: fix
fix:
	@if cargo fixit --version >/dev/null 2>&1; then \
		echo "fix: cargo fixit --clippy"; \
		cargo fixit --clippy --workspace --all-targets --allow-dirty --allow-staged; \
	else \
		echo "fix: cargo clippy --fix (cargo-fixit not installed; cargo install cargo-fixit --locked for ~20x faster repeated runs)"; \
		cargo clippy --fix --workspace --all-targets --allow-dirty --allow-staged -- -D warnings; \
	fi
