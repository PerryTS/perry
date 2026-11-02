Codegen integration-suite exclusions now validate their exact failing tests on
every core pull request, so a fix in HIR, transform, or another dependency
cannot leave a stale exclusion behind. Empty exclusion lists and docs-only
changes continue to skip the Rust toolchain setup.
