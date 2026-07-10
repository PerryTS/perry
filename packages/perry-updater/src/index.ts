// @perry/updater — high-level auto-updater for Perry desktop apps.
//
// Orchestrates: manifest fetch → semver compare → binary download →
// SHA-256 verify → Ed25519 verify → atomic install → detached relaunch,
// plus boot-time rollback on crash-loop detection.
//
// Built on the `perry/updater` ambient primitives (see types/perry/updater)
// and existing `fetch()` + `fs` for the network and disk pieces.

import {
  compareVersions,
  verifyHash,
  verifySignature,
  verifySignatureV2,
  computeFileSha256,
  writeSentinel,
  readSentinel,
  clearSentinel,
  getExePath,
  getSentinelPath,
  installUpdate as nativeInstallUpdate,
  performRollback as nativePerformRollback,
  relaunch as nativeRelaunch,
} from "perry/updater";

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

export interface PlatformAsset {
  url: string;
  sha256: string;
  signature: string;
  size: number;
}

export interface UpdateManifest {
  schemaVersion: number;
  version: string;
  pubDate: string;
  notes: string;
  platforms: { [target: string]: PlatformAsset };
}

export interface Update {
  /** Target version offered by the manifest. */
  version: string;
  /** Release notes (Markdown). */
  notes: string;
  /** Asset metadata for the current platform. */
  asset: PlatformAsset;
  /** Where the staged binary will be written before install. */
  stagedPath: string;
  /** Final target where the running exe lives. */
  targetPath: string;
  /** Download the binary, verifying hash + signature. */
  download(onProgress?: (downloaded: number, total: number) => void): Promise<void>;
  /** Atomically replace the running exe and relaunch detached. Calls process.exit. */
  installAndRelaunch(): Promise<never>;
}

export interface UpdaterOptions {
  /**
   * Manifest URL. **MUST be `https://`** in production. The runtime
   * rejects non-HTTPS URLs except for loopback addresses
   * (`127.0.0.1`, `[::1]`, `localhost`) which are allowed for local
   * smoke tests. See #228 for the threat model.
   */
  manifestUrl: string;
  /** Base64-encoded Ed25519 public key (32 bytes raw). */
  publicKey: string;
  /** Currently-installed version (semver). */
  currentVersion: string;
}

export interface InitOptions {
  /** Auto-rollback after a crash-loop. Default: true. */
  autoRollback?: boolean;
  /** Time after which a fresh install is considered "healthy" and the sentinel is cleared. Default: 60_000 ms. */
  healthCheckMs?: number;
  /** Restart count threshold past which we treat the new version as broken. Default: 2. */
  crashLoopThreshold?: number;
}

// ---------------------------------------------------------------------------
// Platform key
// ---------------------------------------------------------------------------

/**
 * Issue #228: enforce HTTPS on the manifest URL and on each asset URL.
 *
 * Signature pinning protects the integrity of the downloaded code, but
 * an on-path attacker on a non-HTTPS connection can still:
 *   - Suppress legitimate updates (rewrite manifest fetch to claim
 *     "you're up to date").
 *   - Push a downgrade to a still-validly-signed older version that
 *     has a known vulnerability fixed upstream.
 *
 * HTTPS doesn't prevent (2) without #229's version-binding fix, but it
 * raises the bar from "any cafe wifi" to "compromised CA / TLS
 * interception" and stops (1) cold.
 *
 * Loopback addresses (127.0.0.1, [::1], localhost) are allowed for local
 * smoke tests — see scripts/smoke_updater.{sh,ps1} which serve the
 * manifest from http://127.0.0.1:$PORT.
 */
function assertSecureUrl(url: string, kind: "manifest" | "asset"): void {
  let parsed: URL;
  try {
    parsed = new URL(url);
  } catch {
    throw new Error(`updater: ${kind} URL is not a valid URL: ${url}`);
  }
  if (parsed.protocol === "https:") return;
  if (parsed.protocol === "http:") {
    const host = parsed.hostname.toLowerCase();
    // IPv6 hostnames in URL.hostname include the brackets stripped.
    const isLoopback =
      host === "localhost" ||
      host === "127.0.0.1" ||
      host === "::1" ||
      host === "[::1]";
    if (isLoopback) return;
    throw new Error(
      `updater: ${kind} URL must use https:// (got http://${parsed.hostname}). ` +
      `Loopback (127.0.0.1, ::1, localhost) is allowed for local testing.`,
    );
  }
  throw new Error(
    `updater: ${kind} URL must use https:// (got ${parsed.protocol}//).`,
  );
}

function platformKey(): string {
  // os.platform() returns "darwin" / "linux" / "win32"; os.arch() returns
  // "x64" / "arm64" / "ia32". Manifest uses canonical Rust-style triples.
  const platform = (globalThis as any).process?.platform ?? "";
  const arch = (globalThis as any).process?.arch ?? "";
  const os =
    platform === "darwin" ? "darwin" :
    platform === "win32" ? "windows" :
    "linux";
  const a =
    arch === "arm64" ? "aarch64" :
    arch === "x64" ? "x86_64" :
    arch === "ia32" ? "i686" :
    arch;
  return `${os}-${a}`;
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/**
 * Fetch the manifest, compare against `currentVersion`, and return an
 * `Update` handle if a newer version is available for this platform.
 * Returns null when up to date or no asset is published for this platform.
 */
export async function checkForUpdate(opts: UpdaterOptions): Promise<Update | null> {
  assertSecureUrl(opts.manifestUrl, "manifest");
  const res = await fetch(opts.manifestUrl);
  if (!res.ok) {
    throw new Error(`updater: manifest fetch failed: ${res.status}`);
  }
  const manifest = (await res.json()) as UpdateManifest;

  // schemaVersion 1: signed payload is `SHA256(binary)` (legacy, vulnerable to
  // old-binary replay — see #229).
  // schemaVersion 2: signed payload is `SHA256(binary) || version_utf8`
  // (recommended, version-bound).
  if (manifest.schemaVersion !== 1 && manifest.schemaVersion !== 2) {
    throw new Error(`updater: unsupported manifest schemaVersion ${manifest.schemaVersion}`);
  }

  const cmp = compareVersions(opts.currentVersion, manifest.version);
  if (cmp === -2) throw new Error(`updater: invalid version string`);
  if (cmp >= 0) return null; // up to date or downgrade — never offered

  const key = platformKey();
  const asset = manifest.platforms[key];
  if (!asset) return null;

  const targetPath = getExePath();
  const stagedPath = `${targetPath}.staged`;

  return {
    version: manifest.version,
    notes: manifest.notes,
    asset,
    stagedPath,
    targetPath,
    async download(onProgress) {
      await downloadAndVerify(
        asset,
        stagedPath,
        opts.publicKey,
        manifest.version,
        manifest.schemaVersion,
        onProgress,
      );
    },
    async installAndRelaunch() {
      await applyAndRelaunch(stagedPath, targetPath, opts.currentVersion, manifest.version);
      // applyAndRelaunch never returns — process.exit() inside.
      throw new Error("unreachable");
    },
  };
}

/**
 * Boot-time hook: detect failed prior installs and roll back if the new
 * version appears to be crash-looping. Call this near the top of `main()`,
 * right after process initialization.
 *
 * Lifecycle:
 *  - No sentinel:       first boot or clean state → no-op.
 *  - Sentinel "armed":  we are the new version; increment restart count,
 *                       arm a `healthCheckMs` timer to clear the sentinel
 *                       once we look healthy, and register a graceful-exit
 *                       hook so a quick close-and-reopen pattern doesn't
 *                       look like a crash loop.
 *  - Sentinel armed and `restartCount >= crashLoopThreshold`:
 *                       crash loop detected → roll back, clear the captured
 *                       sentinel, and exit only if both operations succeed.
 *                       A failed rollback leaves the sentinel for recovery.
 *
 * The graceful-exit hook is the difference between "user closed the app
 * within 60s" (legitimate, shouldn't bump the count) and "the new version
 * crashed during boot" (should). Without it, short-lived apps and CLIs
 * would false-positive their way into a rollback after two clean
 * close-and-reopen cycles.
 */
export async function initUpdater(options: InitOptions = {}): Promise<void> {
  const autoRollback = options.autoRollback ?? true;
  const healthCheckMs = options.healthCheckMs ?? 60_000;
  const threshold = options.crashLoopThreshold ?? 2;

  // A second initialization (or a disabled re-initialization) supersedes any
  // callbacks registered by the previous generation. Exit hooks cannot be
  // reliably removed on every target, so they also carry this validity bit.
  invalidateActiveLifecycle();

  if (!autoRollback) return;

  const sentinelPath = getSentinelPath();
  const raw = readSentinel(sentinelPath);
  if (!raw) return;

  let state: SentinelPayload;
  try {
    state = JSON.parse(raw) as SentinelPayload;
  } catch {
    // Malformed sentinel — clear it so we don't retry forever.
    clearSentinelOrThrow(sentinelPath, "malformed sentinel");
    return;
  }

  if (state.state !== "armed") return;

  state.restartCount = (state.restartCount ?? 0) + 1;

  if (state.restartCount >= threshold) {
    // Crash loop — roll back and bail.
    // Do not clear or exit when rollback fails: the armed sentinel is the
    // recoverable evidence needed for a later/manual retry. Exiting here
    // would merely recreate the crash loop without restoring the old binary.
    if (nativePerformRollback(getExePath()) !== 1) {
      throw new Error("updater: rollback failed; sentinel retained for recovery");
    }
    clearSentinelIfCurrentOrThrow(sentinelPath, raw, "after rollback");
    (globalThis as any).process?.exit?.(0);
    return;
  }

  // Persist the bumped count, then arm the two paths that can clear the
  // sentinel without triggering a rollback on the next boot:
  //  1. Health-check timer fires after a quiet window — the new version
  //     stayed alive long enough that we trust it.
  //  2. The user gracefully exits before the timer fires — close-and-reopen
  //     is a normal pattern for CLIs and quick-launch GUIs and shouldn't
  //     count toward crash-loop detection.
  // Older sentinels did not include a generation. Upgrade them while writing
  // the bumped restart count so their callbacks get the same protection.
  state.generation = validGeneration(state.generation)
    ? state.generation
    : nextGeneration();
  const capturedSentinel = JSON.stringify(state);
  if (writeSentinel(sentinelPath, capturedSentinel) !== 1) {
    throw new Error("updater: failed to persist crash-loop sentinel");
  }

  const lifecycle: ActiveLifecycle = {
    sentinelPath,
    capturedSentinel,
    active: true,
    timer: undefined,
  };
  activeLifecycle = lifecycle;
  lifecycle.timer = setTimeout(() => {
    clearLifecycleQuietly(lifecycle, "health check");
  }, healthCheckMs);

  // Register a graceful-exit hook if `process.on` is wired in this build.
  // The check keeps initUpdater portable across UI / CLI / minimal targets
  // — if the runtime doesn't expose process events, the timer is the only
  // path and the user can call `markHealthy()` explicitly instead.
  const proc = (globalThis as any).process;
  if (proc && typeof proc.on === "function") {
    proc.on("exit", () => clearLifecycleQuietly(lifecycle, "graceful exit"));
  }
}

/**
 * Explicitly mark the running version as healthy and clear its owned
 * sentinel. Calling this after the sentinel was already cleared is a no-op.
 *
 * `initUpdater` already arms a timer + a graceful-exit hook, so most apps
 * never need to call this. Reach for it when:
 *
 *  - Your app passes its own integrity check earlier than the 60s default
 *    timer (e.g. a successful login, a database migration completed).
 *  - You're on a runtime where `process.on('exit', ...)` isn't wired,
 *    and you want to clear the sentinel on your own shutdown path.
 *  - You're writing a UI app and prefer to call this from `onTerminate`
 *    rather than relying on the generic exit hook.
 */
export function markHealthy(): void {
  const lifecycle = activeLifecycle;
  if (lifecycle?.active) {
    clearLifecycleOrThrow(lifecycle, "markHealthy");
  }
  // No owned lifecycle is also a successful no-op. In particular, an old
  // process must not guess that a sentinel created by a newer install is its
  // own merely because markHealthy was invoked late.
}

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

interface SentinelPayload {
  prevExePath: string;
  stagedAt: string;
  currentVersion: string;
  targetVersion: string;
  restartCount: number;
  state: "installing" | "armed";
  /** Opaque install nonce used to reject stale timer and exit callbacks. */
  generation?: string;
}

interface ActiveLifecycle {
  sentinelPath: string;
  /** Exact on-disk payload this process is allowed to clear. */
  capturedSentinel: string;
  active: boolean;
  timer: ReturnType<typeof setTimeout> | undefined;
}

let activeLifecycle: ActiveLifecycle | undefined;
let generationCounter = 0;

function nextGeneration(): string {
  generationCounter += 1;
  return `${Date.now().toString(36)}-${generationCounter.toString(36)}-${Math.random().toString(36).slice(2)}`;
}

function validGeneration(value: unknown): value is string {
  return typeof value === "string" && value.length > 0;
}

function invalidateActiveLifecycle(): void {
  const lifecycle = activeLifecycle;
  if (!lifecycle) return;
  lifecycle.active = false;
  if (lifecycle.timer !== undefined) clearTimeout(lifecycle.timer);
  activeLifecycle = undefined;
}

/**
 * Compare-and-clear the exact sentinel captured by this lifecycle. This
 * prevents an old timer/exit hook from deleting a newer install's sentinel.
 */
function clearSentinelIfCurrentOrThrow(
  sentinelPath: string,
  expected: string,
  context: string,
): boolean {
  if (readSentinel(sentinelPath) !== expected) return false;
  clearSentinelOrThrow(sentinelPath, context);
  return true;
}

function clearSentinelOrThrow(sentinelPath: string, context: string): void {
  if (clearSentinel(sentinelPath) !== 1) {
    throw new Error(`updater: failed to clear sentinel ${context}`);
  }
}

function clearLifecycleOrThrow(lifecycle: ActiveLifecycle, context: string): void {
  if (!lifecycle.active || activeLifecycle !== lifecycle) return;
  clearSentinelIfCurrentOrThrow(
    lifecycle.sentinelPath,
    lifecycle.capturedSentinel,
    context,
  );
  // A different generation owns the file now, so this lifecycle must never
  // attempt to clear it again. A clear failure intentionally stays active so
  // markHealthy can retry and the sentinel remains recoverable.
  invalidateActiveLifecycle();
}

function clearLifecycleQuietly(lifecycle: ActiveLifecycle, context: string): void {
  try {
    clearLifecycleOrThrow(lifecycle, context);
  } catch (error) {
    // Throwing from a timer or exit hook can turn a healthy process into the
    // very crash loop this sentinel is meant to diagnose. Keep the sentinel
    // for recovery and report the failed cleanup instead.
    (globalThis as any).console?.error?.("updater:", error);
  }
}

async function downloadAndVerify(
  asset: PlatformAsset,
  stagedPath: string,
  publicKey: string,
  version: string,
  schemaVersion: number,
  onProgress?: (downloaded: number, total: number) => void,
): Promise<void> {
  assertSecureUrl(asset.url, "asset");
  const res = await fetch(asset.url);
  if (!res.ok) {
    throw new Error(`updater: download failed: ${res.status}`);
  }
  const total = asset.size;
  // For a streaming-friendly variant use res.body when it's wired; for v1
  // we accept the simpler buffer-the-whole-payload shape since Perry
  // binaries are tens of MB at most.
  const buf = await res.arrayBuffer();
  if (onProgress) onProgress(buf.byteLength, total);

  // Write to staged path atomically: tmp file + rename. Bare fs.writeFileSync
  // is not atomic on its own, so we tmp + rename to make the staged binary
  // appear in one filesystem step.
  //
  // Note: `Buffer.from(arrayBuffer)` is required here. Passing a `Uint8Array`
  // built from the same buffer ends up taking Perry's "string write" path
  // and only the first byte hits disk — separate runtime issue surfaced by
  // the smoke test, easy to step on without realising.
  const fs = await import("fs");
  const tmp = `${stagedPath}.tmp`;
  fs.writeFileSync(tmp, Buffer.from(buf) as any);
  fs.renameSync(tmp, stagedPath);

  if (verifyHash(stagedPath, asset.sha256) !== 1) {
    const actual = computeFileSha256(stagedPath);
    throw new Error(
      `updater: SHA-256 mismatch — expected ${asset.sha256}, got ${actual}`,
    );
  }

  // Issue #229: pick verify path based on manifest schemaVersion.
  // - v1: signature over `SHA256(binary)` only (legacy)
  // - v2: signature over `SHA256(binary) || version_utf8` (binds version
  //       into the signed payload, defeats old-binary replay)
  const sigOk =
    schemaVersion === 2
      ? verifySignatureV2(stagedPath, asset.signature, publicKey, version)
      : verifySignature(stagedPath, asset.signature, publicKey);
  if (sigOk !== 1) {
    throw new Error(
      `updater: Ed25519 signature verification failed (schemaVersion ${schemaVersion})`,
    );
  }
}

async function applyAndRelaunch(
  stagedPath: string,
  targetPath: string,
  currentVersion: string,
  targetVersion: string,
): Promise<void> {
  // This process may have booted the previous installation and armed timer or
  // exit callbacks. Invalidate them before publishing the new generation.
  invalidateActiveLifecycle();

  const sentinelPath = getSentinelPath();
  const prevPath = `${targetPath}.prev`;

  // Arm the sentinel BEFORE we touch the binary. If the install crashes
  // partway, the next boot will see the sentinel and either retry health
  // check or roll back.
  const sentinel: SentinelPayload = {
    prevExePath: prevPath,
    stagedAt: new Date().toISOString(),
    currentVersion,
    targetVersion,
    restartCount: 0,
    state: "armed",
    generation: nextGeneration(),
  };
  const serializedSentinel = JSON.stringify(sentinel);
  if (writeSentinel(sentinelPath, serializedSentinel) !== 1) {
    throw new Error("updater: failed to write sentinel");
  }

  if (nativeInstallUpdate(stagedPath, targetPath) !== 1) {
    clearSentinelIfCurrentOrThrow(sentinelPath, serializedSentinel, "after install failure");
    throw new Error("updater: install failed");
  }

  if (nativeRelaunch(targetPath) < 0) {
    // Relaunch failed — the install already happened, so we're stuck on
    // the new version. Don't roll back here; let the user retry manually.
    throw new Error("updater: relaunch failed (install committed; restart manually)");
  }

  (globalThis as any).process?.exit?.(0);
}
