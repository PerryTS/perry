/**
 * @file Perry publish-pipeline constants. The single place that names the
 *   packages, tap repos, and paths the staged-publish scripts touch, so a
 *   future package or tap change is one edit. Hardcoded to Perry (Perry is
 *   not a fleet member — "generic" means portable structure, not cascade
 *   enrollment).
 */

import path from 'node:path'

/** Repo root = parent of scripts/. */
export const REPO_ROOT = path.resolve(
  path.dirname(new URL(import.meta.url).pathname),
  '..',
  '..',
)

/** Alias used across the publish modules (mirrors a shared `rootPath`). */
export const rootPath = REPO_ROOT

/** The 8 platform packages, staged BEFORE the wrapper (optionalDependencies). */
export const PLATFORM_PACKAGES = [
  '@perryts/perry-darwin-arm64',
  '@perryts/perry-darwin-x64',
  '@perryts/perry-linux-x64',
  '@perryts/perry-linux-arm64',
  '@perryts/perry-linux-x64-musl',
  '@perryts/perry-linux-arm64-musl',
  '@perryts/perry-win32-x64',
  '@perryts/perry-win32-arm64',
] as const

/** The launcher wrapper — staged/approved LAST. */
export const WRAPPER_PACKAGE = '@perryts/perry'

/** All 9, in publish order: platforms first, wrapper last. */
export const ALL_PACKAGES: readonly string[] = [
  ...PLATFORM_PACKAGES,
  WRAPPER_PACKAGE,
]

/** npm dist-tag floor — `latest` is what an untagged install resolves to. */
export const DEFAULT_DIST_TAG = 'latest'

/** The GitHub release repo. */
export const RELEASE_REPO = 'PerryTS/perry'

/** Tap repos the brew/apt legs push to. */
export const HOMEBREW_TAP_REPO = 'PerryTS/homebrew-perry'
export const APT_REPO = 'PerryTS/perry-apt'

/** Path to the curl install script, uploaded as a per-tag release asset. */
export const INSTALL_SH = 'packaging/install.sh'

/** Path to the Homebrew formula the brew tier renders. */
export const HOMEBREW_FORMULA = 'packaging/homebrew/perry.rb'

/** Path to the changelog fragments folder (release notes source). */
export const CHANGELOG_D = 'changelog.d'

/** Shared pipeline state dir (verify/scan/approve receipts). */
export const PIPELINE_STATE_DIR = '.cache/perry/publish-pipeline'

/** Socket org slug for tarball scans. */
export const SOCKET_ORG_SLUG = 'perryts'

/** The Socket scan "repo" label stamped on a staged-upload scan. */
export const SOCKET_SCAN_REPO = 'perry-staged-publish-gate'

/**
 * npm package directories under npm/, in stage order. The launcher lives at
 * npm/perry; each platform package at npm/perry-<suffix>.
 */
export function npmPackageDir(name: string): string {
  // @perryts/perry-darwin-arm64 → npm/perry-darwin-arm64
  // @perryts/perry              → npm/perry
  return `npm/${name.replace(/^@perryts\//, '')}`
}

/**
 * Long-lived npm token env vars the auth-posture gate REFUSES on publish.
 * Publish is OIDC-in-CI + browser-2FA-locally; a long-lived token present
 * during a publish is a defect, not a convenience. Modeled on a tiered registry-infra design
 * auth-posture gate.
 */
export const LONG_LIVED_NPM_TOKEN_ENV_VARS = [
  'NODE_AUTH_TOKEN',
  'NPM_AUTH_TOKEN',
  'NPM_TOKEN',
] as const

/** Read-only registry-read token (granular, read-only scope). Never publishes. */
export const READONLY_TOKEN_ENV = 'PERRY_NPM_READONLY_TOKEN'

/**
 * Minimum npm version for the publish flow. The binding floor is the newest of
 * the features the flow needs:
 *   - `npm stage` (staged publishing)          → npm >= 11.15.0
 *   - OIDC Trusted Publisher                    → npm >= 11.5.1
 *   - `min-release-age` in DAYS (the .npmrc soak) → npm >= 11.17
 *   - `--provenance`                            → npm >= 9.5
 * 11.17.0 covers them all; anything older gets a clear error instead of a
 * cryptic `npm stage` failure.
 */
export const NPM_MIN_VERSION = '11.17.0'
