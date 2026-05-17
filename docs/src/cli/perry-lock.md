# `perry.lock` — Pinned Checksums for Native Library Archives

Every build that consumes a `perry.nativeLibrary` archive with a
`prebuilt:` entry pins the SHA-256 of the resolved archive in a
top-level `perry.lock` file. The next build verifies the hash before
handing the archive to the linker — a swapped or tampered `.a` /
`.lib` / `.dylib` fails the build with a clear diagnostic.

Same model as `package-lock.json`'s `integrity` field, scoped to
exactly the surface perry itself introduced over Node.

**Zero runtime cost.** The check is a compile-time hash + lookup;
the resulting binary is the same size and shape as a build without
the gate.

## File shape

`<project_root>/perry.lock`:

```json
{
  "version": 1,
  "native_libraries": {
    "@bloomengine/engine": {
      "macos-arm64": "sha256:abcd1234..."
    },
    "lodash-native": {
      "linux-x86_64": "sha256:..."
    }
  }
}
```

Per-package + per-target-arch entries. Multi-target builds
accumulate hashes one target at a time — the lockfile is the union
of every target you've ever built.

JSON keys are BTreeMap-sorted so the on-disk file is
byte-deterministic across builds. `git diff perry.lock` is a
meaningful review signal: an unexpected new entry or a hash change
is exactly the supply-chain attack this gate catches.

## Verification semantics

| Lockfile state                | Outcome                                              |
|-------------------------------|------------------------------------------------------|
| No `perry.lock`               | Fresh lockfile written with the current hashes.      |
| Matching entry                | Build proceeds.                                      |
| Missing entry for `(pkg, tgt)`| Added to the lockfile.                               |
| Mismatching entry             | **Build fails** with the package, both hashes, and a one-line fix. |

The diagnostic is verbose by design — the failure is a
reviewer-actionable security event:

```text
Error: archive for `@bloomengine/engine` (target `macos-arm64`)
changed since last accepted:

  expected: sha256:abcd1234...
  found:    sha256:ef015678...
  path:     /repo/node_modules/@bloomengine/engine-darwin-arm64/lib/libbloom.a

Review the package — a swapped or tampered prebuilt static
archive is exactly the supply-chain attack class this lock
was added to catch (#498).

If the change is intentional (dep upgrade, vendored archive
rebuild), rerun the build with `PERRY_LOCK_REFRESH=1` to
rewrite the lockfile entry, or delete `perry.lock` to
regenerate it from scratch.
```

## Updating after a deliberate change

- `PERRY_LOCK_REFRESH=1` — one-time. Mismatched entries are
  rewritten with the current hash, refreshed entries are logged. Use
  this when you've intentionally upgraded a dep or rebuilt a vendored
  archive.
- Delete `perry.lock` — equivalent to a fresh build; every prebuilt
  archive gets re-pinned from scratch.

## CI verification mode

`PERRY_LOCK_FROZEN=1` makes the gate strict in both directions:

- Mismatch → fail (same as default).
- **Missing entry** → fail too (instead of silently adding it).

Set this in CI to ensure every native-library archive has been
explicitly committed to the lockfile. A developer who adds a new
native dep must commit the resulting `perry.lock` update
themselves; CI won't silently extend the lock on their behalf.

## What's NOT covered (deferred follow-ups)

- **Crate-source native libraries** (`crate_path` field, built via
  `cargo`): multi-file builds need a different hash mechanism
  (Cargo.lock + workspace tree hash). The MVP covers single-file
  prebuilt archives only.
- **Cross-target pre-hashing**: hashing all targets a package
  declares at once (instead of accumulating one target per build).
  Useful for CI but not blocking the per-target verification this
  PR provides.
- **`perry lock` CLI subcommand**: env-var refresh + lockfile
  deletion cover the same workflow without a new subcommand.

## See also

- [`#498`](https://github.com/PerryTS/perry/issues/498) — design discussion.
- The wider supply-chain hardening series
  ([`#495`–`#506`](https://github.com/PerryTS/perry/issues?q=is%3Aissue+label%3Aenhancement+security)).
