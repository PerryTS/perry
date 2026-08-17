# Dev-facing targets. Perry is plain cargo (no mise), so this is intentionally
# minimal — add targets here only when a plain `cargo <verb>` isn't enough.

# Apply clippy's machine-applicable autofixes across the workspace. Run on a
# dirty tree is intended (--allow-dirty --allow-staged); review the diff before
# committing. The CI clippy gate is unchanged.
.PHONY: fix
fix:
	@set -e; cargo clippy --fix --workspace --all-targets --allow-dirty --allow-staged -- -D warnings
