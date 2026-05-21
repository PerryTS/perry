#!/usr/bin/env bash
#
# CI gate: fail if any tracked source file exceeds the LOC threshold.
#
# Big single-file modules are hard to read, hard to review, and hurt
# build incrementality (touching one symbol invalidates ~5k lines of
# downstream IDE + cargo-check work). This script enforces an upper
# bound and is run on every PR.
#
# Threshold starts at 5,000 lines (v0.5.1019). The eventual target is
# 2,000 — the codebase has several pre-existing files between 2k and
# 5k that need topical splits before the threshold can drop. Tightening
# happens one file at a time, not in a single sweep, so each tightening
# step is a small reviewable PR.
#
# Excluded outright (not source code):
#   - vendored binaries / .so / .a / .pdf / .png — never source
#   - lock files (Cargo.lock, *.lock, package-lock.json)
#   - translation files (.po / .pot)
#   - generated docs / API references that are regenerated from a manifest
#   - test fixture binaries under tests/modules/
#
# Allowlisted (real source, refactor in progress, tracked separately):
#   - crates/perry-runtime/src/gc/tests.rs — left behind by the gc.rs
#     split in the #1090 GC architecture checkpoint. The companion
#     production files in `gc/` (mod.rs, copying.rs, oldgen.rs, etc.)
#     all stayed under 2k; only the test fixture remained big.
#     Re-evaluate once the GC owner peels it apart.
#
set -euo pipefail

THRESHOLD="${PERRY_FILE_SIZE_THRESHOLD:-5000}"

# Allowlist (one file per line; blank lines + `#` comments OK).
ALLOWLIST=$(cat <<'EOF'
crates/perry-runtime/src/gc/tests.rs
EOF
)

# Anchor at repo root so the script can be invoked from anywhere.
cd "$(git rev-parse --show-toplevel)"

# Build the offender list — tracked files only, skip noise.
violations=""
total=0
while IFS= read -r f; do
    # Skip if not a regular file (deleted, submodule, etc.)
    [ -f "$f" ] || continue

    case "$f" in
        # Generated / non-source artifacts
        Cargo.lock|*.lock|*/Cargo.lock|package-lock.json|yarn.lock|pnpm-lock.yaml) continue ;;
        docs/po/*|*.po|*.pot) continue ;;
        docs/api/perry.d.ts|docs/src/api/reference.md) continue ;;
        docs/runtime-parity.md|docs/runtime-parity-gaps.md) continue ;;
        # Binary or vendored fixtures
        *.so|*.a|*.dylib|*.dll|*.exe|*.o|*.obj) continue ;;
        *.png|*.jpg|*.jpeg|*.gif|*.pdf|*.zip|*.tar|*.gz|*.wasm) continue ;;
        tests/modules/*) continue ;;
        android-build/*/jniLibs/*) continue ;;
        # CHANGELOG.md is a growing log — excluded.
        CHANGELOG.md) continue ;;
    esac

    # Allowlist match.
    if grep -Fxq "$f" <<<"$ALLOWLIST"; then continue; fi

    lines=$(wc -l < "$f" 2>/dev/null || echo 0)
    if [ "$lines" -gt "$THRESHOLD" ]; then
        violations+="$(printf '%7d  %s\n' "$lines" "$f")"$'\n'
        total=$((total + 1))
    fi
done < <(git ls-files)

if [ "$total" -gt 0 ]; then
    echo "::error::File size limit exceeded ($THRESHOLD lines)."
    echo ""
    echo "The following files are too large:"
    echo "$violations"
    echo ""
    echo "Split the offending files into topical sub-modules. See"
    echo "v0.5.1019 for the recipe applied to lower_decl.rs / inline.rs /"
    echo "native_table.rs / collectors.rs / etc.: extract function groups"
    echo "into sibling files, re-export from mod.rs with explicit named"
    echo "use statements (globs don't propagate through transitive"
    echo "re-exports). To deliberately exclude a file (e.g. a refactor"
    echo "in progress tracked elsewhere) add it to the ALLOWLIST block"
    echo "at the top of this script with a one-line rationale."
    exit 1
fi

echo "OK: no source files exceed $THRESHOLD lines."
