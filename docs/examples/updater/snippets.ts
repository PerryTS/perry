// demonstrates: per-API updater snippets shown in docs/src/updater/*.md
// docs: docs/src/updater/overview.md
// platforms: macos, linux, windows
// run: false

// `run: false` because the updater fetches a manifest off the network, replaces
// the running binary on disk, and detached-relaunches — none of which can run
// to a clean exit under the doc-tests harness's 10-second sandboxed timeout.
// We still compile-link the file on every PR, which catches API drift on the
// `perry/updater` ambient and the `@perry/updater` wrapper alike.
//
// Updater is desktop-only (macOS / Windows / Linux). Mobile updates flow
// through the App Store / Play Store and are structurally outside this surface.

// ANCHOR: imports-low-level
import {
    compareVersions,
    verifyHash,
    verifySignature,
    computeFileSha256,
    writeSentinel,
    readSentinel,
    clearSentinel,
    getExePath,
    getBackupPath,
    getSentinelPath,
    installUpdate,
    performRollback,
    relaunch,
} from "perry/updater"
// ANCHOR_END: imports-low-level

// ANCHOR: imports-high-level
// import { checkForUpdate, initUpdater, markHealthy } from "@perry/updater"
// ANCHOR_END: imports-high-level

// ANCHOR: compare-versions
// Returns -1 (current < candidate, update available), 0 (equal),
// 1 (current > candidate, never offered as an update), -2 (parse error).
const cmp = compareVersions("1.4.0", "1.4.1")
if (cmp === -1) {
    console.log("update available")
} else if (cmp === 0) {
    console.log("up to date")
}
// ANCHOR_END: compare-versions

// ANCHOR: verify-file
// SHA-256 + Ed25519 verification of a binary on disk. The signed payload is
// the *raw 32-byte SHA-256 digest* — not the hex string and not the file
// bytes themselves. Sign side: `sha256(file) | ed25519_sign(secret_key)`.
const stagedPath = getExePath() + ".staged"
const expectedHex = "0123456789abcdef..." // from your manifest
const sigB64 = "...base64..."             // 64-byte signature, base64
const pubB64 = "...base64..."             // 32-byte public key, base64

if (verifyHash(stagedPath, expectedHex) !== 1) {
    const actual = computeFileSha256(stagedPath)
    console.error(`hash mismatch — expected ${expectedHex}, got ${actual}`)
}
if (verifySignature(stagedPath, sigB64, pubB64) !== 1) {
    console.error("signature verification failed")
}
// ANCHOR_END: verify-file

// ANCHOR: install-and-relaunch
// `installUpdate` atomically replaces `targetPath` with `stagedPath`,
// keeping the displaced version at `<target>.prev` for rollback.
const target = getExePath()
const staged = target + ".staged"

if (installUpdate(staged, target) !== 1) {
    console.error("install failed")
} else {
    const pid = relaunch(target)
    if (pid < 0) {
        console.error("relaunch failed; restart manually")
    } else {
        // Detached child is now running the new binary — get out of its way.
        process.exit(0)
    }
}
// ANCHOR_END: install-and-relaunch

// ANCHOR: rollback
// `performRollback` restores `<target>.prev` over `target` and moves the
// current (likely-broken) target to `<target>.broken` as a safety net.
if (performRollback(getExePath()) !== 1) {
    console.error("no backup to roll back to")
}
// ANCHOR_END: rollback

// ANCHOR: paths
// Resolved per platform. macOS walks up to the surrounding `.app` bundle;
// Linux honors $APPIMAGE; Windows / bare ELF returns the canonical exe.
console.log("running exe   :", getExePath())
console.log("backup target :", getBackupPath())
// Sentinel path is keyed off PERRY_APP_ID — set this env var so the path
// stays stable across rename/relocation of the binary.
console.log("sentinel path :", getSentinelPath())
// ANCHOR_END: paths

// ANCHOR: sentinel-manual
// Low-level sentinel API. Most apps use `initUpdater()` from @perry/updater
// instead of touching this directly, but it's here when you need it (custom
// rollback policies, multi-process apps, integration with another supervisor).
const sentinelPath = getSentinelPath()
writeSentinel(sentinelPath, JSON.stringify({ state: "armed", restartCount: 0 }))
const raw = readSentinel(sentinelPath)
if (raw) {
    const state = JSON.parse(raw) as { state: string; restartCount: number }
    if (state.restartCount >= 2) {
        // looks like a crash loop — recover or roll back
        clearSentinel(sentinelPath)
    }
}
// ANCHOR_END: sentinel-manual

// ANCHOR: high-level-check
// Pseudocode using @perry/updater's wrapper. Drop into a "Check for updates"
// menu item or a periodic timer. The manifest URL must serve over HTTPS.
//
// const update = await checkForUpdate({
//     manifestUrl: "https://updates.example.com/myapp.json",
//     publicKey: "BASE64_ED25519_PUBKEY",
//     currentVersion: "1.4.0",
// })
//
// if (update !== null) {
//     console.log(`v${update.version} is available`)
//     console.log(update.notes)
//
//     await update.download((downloaded, total) => {
//         const pct = Math.round((downloaded / total) * 100)
//         console.log(`downloading: ${pct}%`)
//     })
//
//     await update.installAndRelaunch()  // never returns — process.exit inside
// }
// ANCHOR_END: high-level-check

// ANCHOR: high-level-init
// Boot-time: detect a crash-looping new install and roll back. Call this
// near the top of `main()`, right after process initialization.
//
// await initUpdater({
//     autoRollback:       true,    // default
//     healthCheckMs:      60_000,  // clear sentinel after this many ms alive
//     crashLoopThreshold: 2,       // restarts before rollback fires
// })
//
// // Optional: tell the updater explicitly that this version is healthy
// // (e.g. after a successful login or migration finished).
// // markHealthy()
// ANCHOR_END: high-level-init

// ANCHOR: manifest-shape
// The manifest is a single JSON file you serve over HTTPS. Each platform
// triple is `<os>-<arch>` (darwin-aarch64, windows-x86_64, windows-aarch64,
// linux-x86_64, linux-aarch64). The wrapper picks the entry matching the
// running host and ignores the rest.
//
// {
//   "schemaVersion": 1,
//   "version": "1.4.0",
//   "pubDate": "2026-04-27T10:00:00Z",
//   "notes": "Bug fixes and performance improvements",
//   "platforms": {
//     "darwin-aarch64": {
//       "url":       "https://example.com/app-1.4.0-darwin-aarch64.bin",
//       "sha256":    "0123456789abcdef...",
//       "signature": "base64sig==",
//       "size":      12345678
//     }
//   }
// }
// ANCHOR_END: manifest-shape

// Suppress unused-var diagnostics in the compile-link gate — every binding
// above is reachable from at least one ANCHOR but the compiler doesn't see
// the .md include sites.
void cmp
void expectedHex
void sigB64
void pubB64
void raw
