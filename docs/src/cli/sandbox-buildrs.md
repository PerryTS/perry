# `PERRY_SANDBOX_BUILDRS`

Wraps `cargo build` invocations triggered by `perry.nativeLibrary`
resolution in macOS `sandbox-exec`, so `build.rs` scripts shipped by
third-party crates can't reach the network or write outside the build
output directory. Build-time only — **zero runtime cost** in the
produced binary. (#505)

## Why

`perry.nativeLibrary` resolution kicks off `cargo build` for any
source-distributed crate. A crate's `build.rs` runs with full developer
privileges, so a typical `bun add @vendor/native-thing` silently grants
the new dependency the ability to exfiltrate environment variables,
read SSH keys, or modify files outside the build tree. The flag flips
that to *opt-out* via an explicit allow-list rather than *opt-in* via
review.

## Opt-in

Off by default for backwards compatibility. Enable per build via
env var:

```bash
PERRY_SANDBOX_BUILDRS=1 perry compile main.ts -o myapp
```

CI typically sets the env var on every job; local development keeps
the legacy flow until ready.

## Profile contents

The generated `sandbox-exec` profile:

- `deny default` + `deny network*` — `build.rs` cannot phone home.
- `allow file-read*` everywhere (cargo / rustc need to read system
  libraries, source, dependency crates).
- `allow file-write*` scoped to `target/`, `~/.cargo`, `~/.rustup`,
  `/tmp`, and the per-build `TempDir`.
- `allow process-fork` + `process-exec` so rustc, cc, ld, and the
  build.rs binaries themselves can run.
- `allow sysctl-read` / `mach-lookup` / `iokit-open` for the platform
  queries cargo and rustc routinely issue.

## Pre-fetch workflow

The sandbox denies network, so cargo cannot reach `crates.io` from
inside it. Pre-fetch once outside the sandbox before the sandboxed
build:

```bash
cargo fetch --manifest-path node_modules/@foo/native-bar/Cargo.toml
PERRY_SANDBOX_BUILDRS=1 perry compile main.ts -o myapp
```

CI runners typically cache `~/.cargo` across jobs, so the pre-fetch is
free on subsequent builds.

## Per-package escape hatch

Some legitimate crates need network during `build.rs` (e.g. fetching
prebuilt artifacts from a CDN). Opt them out per-package in the **host**
`package.json`:

```json
{
  "perry": {
    "allowUnsandboxedBuild": ["@some-vendor/builds-with-network"]
  }
}
```

Host-controlled — transitive deps cannot opt themselves out. The
exemption lives in the host repository's `package.json` and shows up
in code review.

## Cross-platform scope

MVP is macOS-only (the `sandbox-exec` profile). Linux landlock
support is tracked separately; until that lands, `PERRY_SANDBOX_BUILDRS=1`
on Linux is a no-op (the build runs normally). Windows: out of scope.

## See also

- [`#505`](https://github.com/PerryTS/perry/issues/505) — design discussion.
- [`#504`](./emit-attest.md) — companion binary attestation.
- [`#506`](./emit-sandbox.md) — companion runtime sandbox profile.
