// Type declarations for perry/updater — Perry's auto-update primitives.
//
// This module exposes the security-critical and platform-touching pieces
// (semver compare, hash + Ed25519 signature verification, atomic install,
// sentinel-based rollback, detached relaunch). The download itself is left
// to the TS layer using the existing `fetch()` API — see `@perry/updater`
// for the high-level Tauri-style wrapper around these primitives.

/**
 * Compare two semver versions.
 *
 * Returns -1 if `current < candidate` (an update is available),
 *         0 if equal,
 *         1 if `current > candidate`,
 *        -2 if either input fails to parse.
 */
export function compareVersions(current: string, candidate: string): number;

/**
 * Verify the SHA-256 digest of a file matches an expected lowercase hex string.
 * Returns 1 on match, 0 on any failure (file missing, mismatch, unreadable).
 */
export function verifyHash(filePath: string, expectedHex: string): number;

/**
 * Compute the SHA-256 hex digest of a file. Returns empty string on failure.
 * Useful for logging the actual hash on a `verifyHash` mismatch.
 */
export function computeFileSha256(filePath: string): string;

/**
 * Verify an Ed25519 signature over the SHA-256 digest of a file.
 *
 * The signed payload is the **raw 32-byte SHA-256 digest** of the file
 * (NOT the hex string, NOT the file bytes themselves). Sign side must
 * compute SHA-256 → sign the raw 32 bytes with the Ed25519 secret key.
 *
 * **Use only with manifest schemaVersion 1**. Manifests at
 * schemaVersion 2+ MUST use `verifySignatureV2` so the version is
 * bound into the signed payload (see #229 — old-binary replay defense).
 *
 * @param sigB64    base64-encoded 64-byte signature
 * @param pubkeyB64 base64-encoded 32-byte public key
 * Returns 1 on valid signature, 0 on any error (size, decode, mismatch).
 */
export function verifySignature(
  filePath: string,
  sigB64: string,
  pubkeyB64: string,
): number;

/**
 * **Issue #229**: version-bound Ed25519 signature verification.
 *
 * The signed payload is **`SHA256(file) || version_utf8`** — 32 bytes
 * of raw digest followed by the UTF-8 bytes of the version string.
 * This binds the version into the signature so an on-path attacker
 * can't replay a previously-signed older binary as a "new version" by
 * pairing the old binary's URL + signature with a higher version
 * number in a tampered manifest.
 *
 * Manifest schemaVersion 2+ uses this path. v1 manifests stay on
 * `verifySignature` (digest-only); the @perry/updater wrapper
 * dispatches based on `manifest.schemaVersion`.
 *
 * Sign side: `payload = sha256(binary).digest() + version.encode("utf-8")`,
 * then sign `payload` with the Ed25519 secret key.
 *
 * Empty `version` is rejected (would let a v1 signature pass v2 verify).
 *
 * @param sigB64    base64-encoded 64-byte signature over digest||version
 * @param pubkeyB64 base64-encoded 32-byte public key
 * @param version   UTF-8 version string — exact bytes the manifest claims
 * Returns 1 on valid signature, 0 on any error.
 */
export function verifySignatureV2(
  filePath: string,
  sigB64: string,
  pubkeyB64: string,
  version: string,
): number;

/**
 * Atomically write `payload` to `sentinelPath`, creating the parent directory
 * if needed. Returns 1 on success, 0 on any IO error.
 */
export function writeSentinel(sentinelPath: string, payload: string): number;

/**
 * Read the sentinel file. Returns the contents as a string, or empty string
 * if the file is missing/unreadable. The caller (TS) parses the JSON.
 */
export function readSentinel(sentinelPath: string): string;

/**
 * Delete the sentinel file. Returns 1 on success or if the file did not
 * exist to begin with (idempotent), 0 on any other IO error.
 */
export function clearSentinel(sentinelPath: string): number;

// --- Desktop platform helpers (the `desktop` module of perry-updater) ---

/**
 * Resolve the path to the running executable, accounting for platform quirks:
 * - macOS: returns the surrounding `.app` bundle path if applicable.
 * - Linux: honors `$APPIMAGE` when set (the AppImage runtime points
 *   `current_exe()` inside a read-only squashfs mount).
 * - Windows / bare ELF / bare Mach-O: returns the canonical exe path.
 */
export function getExePath(): string;

/**
 * Sibling backup path: `<exe>.prev`. This is where `installUpdate` keeps the
 * previous version so `performRollback` can restore it.
 */
export function getBackupPath(): string;

/**
 * Per-OS user-writable sentinel path:
 * - macOS:   `~/Library/Application Support/<app>/updater.sentinel`
 * - Windows: `%LOCALAPPDATA%\<app>\updater.sentinel`
 * - Linux:   `$XDG_STATE_HOME/<app>/updater.sentinel`
 *
 * `<app>` comes from `PERRY_APP_ID` env var, falling back to the basename
 * of the running exe. Apps SHOULD set `PERRY_APP_ID` so the path stays
 * stable across rename/relocation.
 */
export function getSentinelPath(): string;

/**
 * Atomically replace `targetPath` with `stagedPath`, keeping the displaced
 * version at `<target>.prev`. On Unix, ensures the new binary has 0o755
 * permissions. Returns 1 on success, 0 on any IO error (and attempts to
 * roll back step 1 on a failed step 2).
 */
export function installUpdate(stagedPath: string, targetPath: string): number;

/**
 * Restore `<target>.prev` over `target`, undoing a prior install.
 * Moves the current (likely-broken) target to `<target>.broken` first as
 * a safety net. Returns 1 on success, 0 if no backup exists.
 */
export function performRollback(targetPath: string): number;

/**
 * Spawn `exePath` as a fully detached child process and return the child's
 * PID, or -1 on failure. The caller is expected to call `process.exit(0)`
 * shortly after — that's how the running process hands off to the new one.
 */
export function relaunch(exePath: string): number;

// ---------------------------------------------------------------------------
// The embedded `perry.update` block, for an app that carries its own check.
//
// Configure it in package.json and Perry bakes it into the binary; the runtime
// notifies from what a previous run recorded. These are how the app performs
// the check itself: the runtime says what to request and reads the answer, and
// the app's own `fetch()` makes the request. Doing it that way keeps an HTTP
// stack out of every compiled binary that never checks for an update.
//
// See docs/src/cli/app-updates.md.
// ---------------------------------------------------------------------------

/**
 * The app's embedded update settings as a JSON string, or `""` when the project
 * configured none.
 *
 * The exact bytes the compiler wrote, so the two sides cannot disagree about a
 * field name.
 */
export function getEmbeddedConfig(): string;

/**
 * The URL to request for the configured source, or `""` when no request should
 * be made.
 *
 * Empty is a decision, not a failure: a `gh-registry` source with no token
 * would 404, and a 404 reads as "up to date", so the runtime declines to name a
 * URL rather than letting the app report itself current forever.
 *
 * @param token A GitHub token for `gh-registry`. Pass `undefined` otherwise —
 *   it is never sent to the public npm registry.
 */
export function embeddedCheckUrl(token?: string): string;

/**
 * The headers for that request, as `name: value` lines separated by newlines,
 * or `""` when there are none.
 */
export function embeddedCheckHeaders(token?: string): string;

/**
 * Interpret a response body according to the configured source and record what
 * it said. Returns 1 when something was recorded, 0 otherwise.
 *
 * Parsing stays in the runtime so the four source shapes agree with the
 * compiler that emitted the block, and so an app cannot record a version its
 * source never named. A body that does not match the configured shape, or whose
 * version does not parse, records nothing.
 */
export function recordEmbeddedResponse(body: string): number;

/**
 * Whether enough time has passed since the last check, per the configured
 * `checkInterval`. Returns 1 when a request is due.
 *
 * Call this before fetching, so an app does not ask its registry on every run.
 */
export function embeddedRefreshDue(): number;
