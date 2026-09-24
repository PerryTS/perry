Replace narrow Rust utility dependencies with focused Perry helpers: CLI logging,
UTC formatting, Apple JWT framing, progress and dotenv parsing; strict hex/base64
codecs; v4/v7 UUID generation; and DNS nameserver configuration. Migrate direct
lazy_static/once_cell uses to std lazy initialization.

Select helpers through explicit Cargo features: Apple signing is absent from the
dev CLI, UUID generation is absent from minimal stdlib builds, and uncommon ZIP
codecs require `extended-zip`. Keep RustCrypto signing primitives and existing
runtime feature detection. Fix minimal stdlib's worker stack-size helper so it
no longer depends on the optional async bridge.

Add compatibility tests and CI dependency-boundary checks. Against the PR base,
the macOS compiler graph drops from 339 to 287 packages (dev CLI: 331 to 255).
See `docs/audits/lean-dependencies.md` for graph methodology and remaining shared
transitive dependencies. No version bump, as requested.
