# Dev-facing targets. Perry is plain cargo (no mise), so this is intentionally
# minimal — add targets here only when a plain `cargo <verb>` isn't enough.
RUSTUP_TOOLCHAIN := $(shell sed -n 's/^channel = "\(.*\)"/\1/p' rust-toolchain.toml)
CARGO_BIN := $(if $(CARGO_HOME),$(CARGO_HOME),$(HOME)/.cargo)/bin
LLVM_SYS_221_PREFIX ?= $(shell brew --prefix llvm@22 2>/dev/null)

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
	@set -e; PATH="$(CARGO_BIN):$$PATH" cargo fixit --clippy --workspace --all-targets --allow-dirty --allow-staged

# Install the pinned cargo-fixit dev tool that `make fix` drives. cargo-fixit
# is a cargo crate with NO prebuilt binaries (no cargo binstall, no GitHub
# release assets), so it is pinned in external-tools.json under the
# `cargo-install` release type and built from source with `--locked`.
# The version is read from external-tools.json so the pin has one source of truth.
.PHONY: fix-deps
fix-deps:
	@set -e; v=$$(node -e "console.log(require('./external-tools.json').tools['cargo-fixit'].version)"); \
	rustup run $(RUSTUP_TOOLCHAIN) cargo install cargo-fixit@$$v --locked

# Install the pinned Mr Boxington Cargo cache used by build-dev/build-prod.
.PHONY: mbx-deps
mbx-deps:
	@set -e; v=$$(node -e "console.log(require('./external-tools.json').tools.mbx.version)"); \
	rustup run $(RUSTUP_TOOLCHAIN) cargo install mbx@$$v --locked

# Local iteration. The dev profile prioritizes quick, debuggable builds;
# prod uses opt-level 3 and ThinLTO with parallel codegen units. Leave
# MBX_INCREMENTAL unset: mbx keeps learned
# incremental state for edited crates while retaining shared-cache reuse for
# unchanged dependencies. Explicit Cargo incremental mode disables that sharing.
.PHONY: build-dev build-fast build-prod
build-dev:
	PATH="$(CARGO_BIN):$$PATH" LLVM_SYS_221_PREFIX="$(LLVM_SYS_221_PREFIX)" mbx build --profile dev -p perry

build-fast: build-dev

build-prod:
	PATH="$(CARGO_BIN):$$PATH" LLVM_SYS_221_PREFIX="$(LLVM_SYS_221_PREFIX)" mbx build --profile prod -p perry

# Dedicated Linux agent hosts must provision compressed, quota-bounded ZFS
# before building. Both paths are explicit to avoid using the root filesystem.
.PHONY: build-agent
build-agent:
	python3 scripts/check_zfs_build_storage.py
	$(MAKE) build-dev
