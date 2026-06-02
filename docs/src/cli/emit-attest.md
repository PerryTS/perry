# `--emit-attest` (binary attestation sidecar)

`perry compile --emit-attest main.ts -o myapp` writes
`myapp.attest.json` next to the executable. The sidecar holds the
SHA-256 of the *post-strip / post-codesign* binary plus provenance
metadata (perry version, git commit, build timestamp) so downstream
consumers can verify that the artifact they downloaded matches the
one the publisher built. (#504)

## Why

Publishing a Perry binary to a CDN, a release page, or an internal
artifact registry creates a window between "publisher built it" and
"user runs it." `--emit-attest` produces a JSON sidecar that anyone
can recompute on the downloaded artifact and compare. A tampered or
swapped binary fails verification with a verbose diagnostic that
reproduces both hashes.

## Emit

```bash
perry compile --emit-attest main.ts -o myapp
# → myapp
# → myapp.attest.json
```

Equivalent settings, last wins:

1. `perry.emitAttest: true` in host `package.json`.
2. `PERRY_EMIT_ATTEST=1` in the environment.
3. `--emit-attest` on the CLI.

`=0` / `false` explicitly disables (so a CI matrix can override a
host-level opt-in).

## Verify

```bash
perry verify --attest ./myapp
```

Streams SHA-256 of the binary on disk and compares against
`myapp.attest.json`. Output:

- **match** — prints `✓ attestation matches` plus the captured
  provenance (perry version, commit SHA, build timestamp). Exit 0.
- **mismatch** — prints both hashes, the sidecar's provenance, and
  exit 1.
- **missing sidecar** — prints actionable guidance pointing at
  `--emit-attest`. Exit 1.

The verifier runs offline (no tokio runtime, no network, no beta
consent prompt) — distinct from the existing
`perry verify` which goes through `verify.perryts.com` for runtime
verification.

## Manifest shape

```json
{
  "version": 1,
  "sha256": "abcd1234...",
  "size": 1048576,
  "perry_version": "0.5.999",
  "commit_sha": "0a1b2c3...",
  "built_at_unix": 1715990400,
  "binary_filename": "myapp"
}
```

`version: 1` reserves room for future top-level keys (CI signature
blob, sigstore bundle, reproducible-builds flags log) without
breaking existing parsers.

## When the hash is captured

The hash is computed *after* every post-link rewrite the platform
applies — `strip`, `codesign`, `install_name_tool` retag, ad-hoc
extended-attribute scrubs. That's the same byte sequence users
download, so the recomputed hash matches when the artifact is
intact.

## Cross-platform

The hook lives in the platform-agnostic `compile_command` driver,
so every backend (LLVM, WASM, ArkTS, HarmonyOS, Glance, SwiftUI,
JS) emits the sidecar consistently.

## Follow-ups (MVP scope)

The MVP captures hash + provenance. Full reproducible-builds and
sigstore-style remote signature publication are tracked separately
under the same issue.

## See also

- [`#504`](https://github.com/PerryTS/perry/issues/504) — design discussion.
- [`#505`](./sandbox-buildrs.md) — companion build-time sandbox.
- [`#506`](./emit-sandbox.md) — companion runtime sandbox profile.
