# Auto-Update

Ship updates to your Perry desktop app without rolling your own download +
replace + relaunch flow. Two modules cooperate:

- **`@perry/updater`** — high-level wrapper. The 90% case: manifest fetch,
  semver compare, download, verify, install, relaunch, crash-loop rollback.
- **`perry/updater`** — ambient primitives the wrapper is built on. Reach
  for these only when you need a custom flow (multi-channel rollouts, your
  own progress UI, an integration with an external supervisor).

The trust model and wire format follow [Tauri's updater](https://v2.tauri.app/plugin/updater/):
JSON manifest over HTTPS, SHA-256 + Ed25519 signature over the digest, atomic
binary replace with a `.prev` backup, detached relaunch. Every snippet below
is excerpted from
[`docs/examples/updater/snippets.ts`](../../examples/updater/snippets.ts) — CI
compile-links it on every PR.

> **Desktop only.** iOS / TestFlight, Android Play Store, and sideloaded APKs
> own the install pipeline at the OS level — replacing your own binary at
> runtime is structurally impossible there. The crate still compiles on
> mobile targets so cross-platform code doesn't need `#ifdef`s, but the
> install path is a no-op. Gate updater code with `process.platform` if your
> app ships everywhere.

## Quick start

```typescript
{{#include ../../examples/updater/snippets.ts:imports-high-level}}
```

Drop a "Check for updates" handler somewhere in your menu or a periodic
timer. `checkForUpdate` returns `null` when up to date or when the manifest
has no entry for the current platform.

```typescript
{{#include ../../examples/updater/snippets.ts:high-level-check}}
```

Call `initUpdater()` once near the top of `main()`. It handles boot-time
crash-loop detection: if the new binary you just installed crashes during
boot more than `crashLoopThreshold` times, the wrapper restores the previous
version and exits so the OS / launcher restarts you on the rollback.

```typescript
{{#include ../../examples/updater/snippets.ts:high-level-init}}
```

## Manifest

Serve a single JSON file over HTTPS. One entry per `<os>-<arch>` you
publish for; clients ignore entries that don't match their platform.

```typescript
{{#include ../../examples/updater/snippets.ts:manifest-shape}}
```

| Field | Meaning |
|-------|---------|
| `schemaVersion` | `1` (legacy, digest-only signature) or `2` (recommended — version-bound signature; see below). |
| `version` | Semver string of the offered version (e.g. `"1.4.0"`). |
| `pubDate` | ISO-8601 timestamp the build was published — surfaced as metadata. |
| `notes` | Markdown release notes shown to the user. |
| `platforms.<os>-<arch>.url` | Direct download URL (HTTPS). |
| `platforms.<os>-<arch>.sha256` | Lowercase hex SHA-256 of the binary. |
| `platforms.<os>-<arch>.signature` | Base64 Ed25519 signature. v1: over the raw 32-byte digest. v2: over `digest \|\| version_utf8`. |
| `platforms.<os>-<arch>.size` | Byte length of the binary — used for progress reporting. |

Platform keys are canonical Rust-style triples:

| Host | Triple |
|------|--------|
| Apple Silicon mac | `darwin-aarch64` |
| Intel mac | `darwin-x86_64` |
| Windows 10/11 64-bit | `windows-x86_64` |
| Linux 64-bit | `linux-x86_64` |
| Linux ARM64 | `linux-aarch64` |

## Trust model

### `schemaVersion: 2` (recommended) — version-bound signature

The signed payload is `SHA256(binary) || version_utf8` — the 32-byte
raw digest concatenated with the UTF-8 bytes of the version string.
This binds the version into the signature, so an on-path attacker
can't replay a previously-signed older binary as a "new version" by
serving a manifest that pairs the old binary's URL + signature with a
higher version number ([#229](https://github.com/PerryTS/perry/issues/229)).

Sign side:

```text
payload = sha256(binary).digest() + version.encode("utf-8")
signature = ed25519_sign(secret_key, payload)  # 64-byte signature
```

Verification on the client:

1. SHA-256 the downloaded file. Reject if it doesn't match `manifest.sha256`.
2. Build the v2 payload (`digest || version_utf8`) using `manifest.version`.
3. Ed25519-verify the signature against the bundled public key.
4. Reject on any decode error, size mismatch, or signature failure.

If an attacker swaps the manifest's `version` field while keeping the
old `signature`, step 3 fails because the signature was made over the
*original* version. If they swap both `version` and `signature` (using
a previously-signed older binary), step 1 fails because `sha256` of
the older binary doesn't match the rewritten higher-version label
either — every plausible attack on the version metadata invalidates
something.

### `schemaVersion: 1` (legacy, digest-only)

The signed payload is the raw 32-byte digest only. **This shape is
vulnerable to old-binary replay** ([#229](https://github.com/PerryTS/perry/issues/229)):
an on-path attacker can serve a manifest claiming a higher version
while pointing at a previously-signed older binary, and signature
verification still passes because the version isn't bound into the
signature. Existing v1 manifests stay supported by the client during
migration; new deployments should use v2.

### Migration

`@perry/updater` v0.5.391+ accepts both `schemaVersion: 1` and
`schemaVersion: 2`. Bumping your manifest from 1 → 2 requires:

1. Update sign-side tooling to compute the new payload
   (`sha256(binary) + version.encode("utf-8")`) and sign that.
2. Bump `schemaVersion` in the manifest from `1` to `2`.
3. Make sure all deployed clients are running a perry-updater version
   that knows about v2 BEFORE you publish a v2 manifest. Older clients
   reject `schemaVersion: 2` with an `unsupported manifest schemaVersion`
   error. (Plan: ship a perry-updater bump with v2 support to your
   users via a v1 manifest first; once they're on the v2-aware client,
   the next manifest can be v2.)

### Keypair

You generate the keypair once and bake the public key into your app at
build time; the secret key stays on a release-signing machine alongside
the rest of your build artifacts. Compromise of the manifest server alone
never lets an attacker push a binary your client will accept.

### Sign-side CLI (v0.5.395+)

`perry updater` ships sign-side subcommands without needing custom tooling.
`keygen`, `sign`, and `verify` serve the desktop updater's v2 artifact format;
`sign-cli-manifest` signs the authenticated manifest consumed by `perry update`.

```bash
# 1) One-time keypair generation. Save kp.json with mode 0600 and
#    NEVER commit the secret_key field.
perry updater keygen --output kp.json

# 2) Per-release signing. The output JSON envelope contains every
#    manifest-entry field (sha256, signature, size, version, schemaVersion=2).
perry updater sign \
    --binary perry-darwin-aarch64.tar.gz \
    --version 1.2.3 \
    --secret-key kp.json

# 3) Sanity-check the signature locally before uploading the manifest —
#    this is the same algorithm the runtime uses, so a passing verify
#    here predicts a passing verify on the client.
perry updater verify \
    --binary perry-darwin-aarch64.tar.gz \
    --version 1.2.3 \
    --signature '<base64 from step 2>' \
    --pubkey '<public_key from kp.json>'

# 4) Release CI signs the CLI manifest, binding its archive URL, platform,
# version, SHA-256, size, and key id into the Ed25519 signature.
perry updater sign-cli-manifest \
    --artifact perry-linux-x86_64.tar.gz \
    --platform perry-linux-x86_64.tar.gz \
    --url 'https://github.com/PerryTS/perry/releases/download/v1.2.3/perry-linux-x86_64.tar.gz' \
    --release-version 1.2.3 \
    --key-id release-2026 \
    --secret-key-b64 "$PERRY_CLI_UPDATE_SIGNING_KEY"
```

Compose a final manifest by piping `sign` output through `jq` for each
asset, then merging into the per-platform layout shown above. CI tip:
pass `--secret-key-b64 "$ED25519_SECRET_KEY"` instead of `--secret-key
file` so the secret can come from a repository secret without ever
hitting the worker's filesystem.

## Install + rollback flow

```text
manifest fetch  →  semver compare  →  download to <exe>.staged
                                          ↓
                                      sha256 verify  →  ed25519 verify
                                          ↓
                                      arm sentinel (state: "armed")
                                          ↓
                                      install:  rename <exe> → <exe>.prev
                                                rename <exe>.staged → <exe>
                                                chmod +x   (Unix only)
                                          ↓
                                      detached relaunch  →  process.exit(0)

next boot  →  initUpdater() reads sentinel
                  ├── healthCheckMs alive  → clearSentinel  (success)
                  ├── graceful exit         → clearSentinel  (success)
                  └── restartCount ≥ N      → performRollback + exit
```

`installUpdate` is atomic where the OS lets us be: POSIX `rename(2)` on the
same filesystem, NTFS rename-while-open (the PE loader opens with
`FILE_SHARE_DELETE` so Windows tolerates this since Vista), and Linux's
mmap'd-inode-stays-alive semantics. If the staging directory ends up on a
different filesystem (a separate mount for `/tmp`, for instance) the rename
falls back to copy + remove, which has a small non-atomic window.

The sentinel is a JSON file at a per-OS user-writable path:

| Platform | Default location |
|----------|------------------|
| macOS | `~/Library/Application Support/<app>/updater.sentinel` |
| Windows | `%LOCALAPPDATA%\<app>\updater.sentinel` |
| Linux | `$XDG_STATE_HOME/<app>/updater.sentinel` |

`<app>` comes from the `PERRY_APP_ID` environment variable, falling back
to the basename of the running exe. **Set `PERRY_APP_ID` in your launch
environment** so the sentinel path stays stable across rename / relocation
of the binary.

## Low-level primitives

Use these when the high-level wrapper doesn't fit — custom progress UI, a
multi-channel manifest, an external supervisor that handles restarts, etc.

```typescript
{{#include ../../examples/updater/snippets.ts:imports-low-level}}
```

### `compareVersions(current, candidate)`

Returns `-1` (update available), `0` (equal), `1` (downgrade — never offered),
or `-2` (parse error). Prerelease tags handled per the semver spec.

```typescript
{{#include ../../examples/updater/snippets.ts:compare-versions}}
```

### `verifyHash` / `verifySignature` / `computeFileSha256`

```typescript
{{#include ../../examples/updater/snippets.ts:verify-file}}
```

`verifyHash` and `verifySignature` return `1` on success, `0` on any failure
(file missing, decode error, mismatch). `computeFileSha256` returns the hex
digest as a string, or `""` on failure — useful for logging the *actual* hash
when a `verifyHash` mismatch fires.

### `installUpdate` / `performRollback` / `relaunch`

```typescript
{{#include ../../examples/updater/snippets.ts:install-and-relaunch}}
```

```typescript
{{#include ../../examples/updater/snippets.ts:rollback}}
```

`relaunch` returns the child PID, or `-1` on failure. The new process is
fully detached (`setsid` on Unix, `DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP`
on Windows) so closing the current process doesn't take it down.

### Path resolution

```typescript
{{#include ../../examples/updater/snippets.ts:paths}}
```

`getExePath()` accounts for platform quirks:

- **macOS**: walks up to the surrounding `.app` bundle if applicable —
  the `.app` directory is the codesign unit, so that's what you replace.
- **Linux**: honors `$APPIMAGE` when set. The AppImage runtime points
  `current_exe()` inside a read-only squashfs mount; the real target to
  replace is the AppImage file itself.
- **Windows / bare ELF / bare Mach-O**: returns the canonicalized exe path.

### Sentinel

```typescript
{{#include ../../examples/updater/snippets.ts:sentinel-manual}}
```

`writeSentinel` is atomic (tmp file + rename), creates the parent directory
if needed, and returns `1` on success / `0` on any IO error.
`clearSentinel` is idempotent — returns `1` whether the file existed or not.

## What's not here (yet)

- **UI primitives** — a "Restart now" modal with `ProgressView` belongs
  inside `perry/ui` proper rather than the updater package. Tracked as a
  follow-up.
- **Privileged install** for system-wide locations (`/Applications`,
  `Program Files`). The current install path only handles user-writable
  locations (`~/Applications`, `~/.local/bin`, `%LOCALAPPDATA%`).
  UAC / `SMJobBless` is a separate concern.
- **Delta updates** (bsdiff), **multi-channel** (stable / beta), **staged
  rollouts**.
- **Notarization / code-signing** during install. Binaries are expected to
  arrive already signed; the updater doesn't try to be a notarization tool.

## Testing your update flow

The crate ships smoke-test scripts that exercise verify → install → relaunch
end-to-end against a real Perry binary:

- **Unix**: `scripts/smoke_updater.sh`
- **Windows**: `scripts/smoke_updater.ps1`

Both spin up a tiny HTTP server, build a v1.0.0 binary that drives the
update flow, build a v1.0.1 binary that proves it ran, and verify the
relaunch handed off correctly. Run them locally before shipping a release
that depends on the updater wiring.

## Next Steps

- [System APIs](../system/overview.md) — the rest of `perry/system`
- [HTTP & Networking](../stdlib/http.md) — `fetch()` is what the wrapper
  uses internally
- [Cryptography](../stdlib/crypto.md) — Ed25519 sign / verify primitives
  for tooling that builds the manifest
