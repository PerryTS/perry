/**
 * @file Perry publish-pipeline constants. The single place that names the
 *   packages, tap repos, and paths the publish scripts touch, so a
 *   future package or tap change is one edit. Hardcoded to Perry (Perry is
 *   not a fleet member — "generic" means portable structure, not cascade
 *   enrollment).
 */

import { readFileSync } from 'node:fs'
import path from 'node:path'

/** Repo root = parent of scripts/. */
export const REPO_ROOT = path.resolve(
  path.dirname(new URL(import.meta.url).pathname),
  '..',
  '..',
)

/** Alias used across the publish modules (mirrors a shared `rootPath`). */
export const rootPath = REPO_ROOT

/** The 8 platform packages, published BEFORE the wrapper (optionalDependencies). */
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

/**
 * The triples perry publishes a package for, from the one list
 * `npm/packaged-triples.json`. perry compiles the same file in
 * (`target_packages::is_packaged`), so a triple listed here is also the one
 * perry names in its install hints and fetches; stage-npm.sh reads it
 * through `--print-targets` / `--print-ext`.
 */
interface PackagedTriples {
  targets: string[]
  ext: string[]
}
const PACKAGED_TRIPLES = JSON.parse(
  readFileSync(path.join(REPO_ROOT, 'npm', 'packaged-triples.json'), 'utf8'),
) as PackagedTriples

/**
 * Cross targets that ship as their own host-independent npm package, one per
 * Rust triple (the triple perry's `rust_target_triple` returns). Each is
 * staged by scripts/stage-npm.sh from the `perry-cross-<triple>.tar.gz`
 * bundle; perry finds an installed one next to its own binary.
 */
export const TARGET_TRIPLES: readonly string[] = Object.freeze([...PACKAGED_TRIPLES.targets])

/**
 * Hosts whose native-extension archives (`libperry_ext_*.a`, ~140-166 MB
 * packed) ship as `@perryts/perry-ext-<triple>`: too big for the host
 * package, and not a dependency of the wrapper (most programs never link
 * one). perry fetches the matching one from the GitHub release the first
 * time a compile needs it, or finds it npm-installed beside itself. Each is
 * staged by stage-npm.sh from that host's release archive.
 */
export const EXT_TRIPLES: readonly string[] = Object.freeze([...PACKAGED_TRIPLES.ext])

/** `@perryts/perry-target-<triple>`: the name is derived, never tabulated. */
export function targetPackageName(triple: string): string {
  return `@perryts/perry-target-${triple}`
}

/** `@perryts/perry-ext-<triple>`: the name is derived, never tabulated. */
export function extPackageName(triple: string): string {
  return `@perryts/perry-ext-${triple}`
}

/** The target packages, published with the platforms, before the wrapper. */
export const TARGET_PACKAGES: readonly string[] = TARGET_TRIPLES.map(targetPackageName)

/** The ext packages, published with the platforms, before the wrapper. */
export const EXT_PACKAGES: readonly string[] = EXT_TRIPLES.map(extPackageName)

/** The launcher wrapper — published LAST. */
export const WRAPPER_PACKAGE = '@perryts/perry'

/**
 * Every package, in publish order: platforms, then ext packages, then
 * targets, then the wrapper. The wrapper depends on neither ext nor target
 * packages, but it must not become visible before them: platform-visibility.mjs
 * waits on every non-wrapper entry of the publish manifest before the
 * wrapper is published.
 */
export const ALL_PACKAGES: readonly string[] = [
  ...PLATFORM_PACKAGES,
  ...EXT_PACKAGES,
  ...TARGET_PACKAGES,
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

/** Shared pipeline state dir (CI proof/registry/release receipts). */
export const PIPELINE_STATE_DIR = '.cache/perry/publish-pipeline'

/** Socket org slug for tarball scans. */
export const SOCKET_ORG_SLUG = 'perryts'

/** The Socket scan "repo" label stamped on an npm publish-candidate scan. */
export const SOCKET_SCAN_REPO = 'perry-npm-publish-gate'

/**
 * npm package directories under npm/, in stage order. The launcher lives at
 * npm/perry; each platform package at npm/perry-<suffix>; each target package
 * at npm/perry-target-<triple> (generated whole from npm/perry-target/); each
 * ext package at npm/perry-ext-<triple> (generated whole from npm/perry-ext/).
 */
export function npmPackageDir(name: string): string {
  // @perryts/perry-darwin-arm64 → npm/perry-darwin-arm64
  // @perryts/perry-target-wasm32-wasip2 → npm/perry-target-wasm32-wasip2
  // @perryts/perry-ext-x86_64-unknown-linux-gnu → npm/perry-ext-x86_64-unknown-linux-gnu
  // @perryts/perry              → npm/perry
  return `npm/${name.replace(/^@perryts\//, '')}`
}

/**
 * Long-lived npm token env vars the auth-posture gate REFUSES on publish.
 * Publish is OIDC-in-CI only; a long-lived token present during a publish is a
 * defect, not a convenience. Modeled on a tiered registry-infra design
 * auth-posture gate.
 */
export const LONG_LIVED_NPM_TOKEN_ENV_VARS = [
  'NODE_AUTH_TOKEN',
  'NPM_AUTH_TOKEN',
  'NPM_TOKEN',
] as const

/**
 * Minimum npm version for the CI publish flow. The binding floor is the newest
 * of the features the flow needs:
 *   - OIDC Trusted Publisher                    → npm >= 11.5.1
 *   - `min-release-age` in DAYS (the .npmrc soak) → npm >= 11.17
 *   - `--provenance`                            → npm >= 9.5
 * 11.17.0 covers them all; anything older gets a clear error instead of a
 * cryptic Trusted Publisher failure.
 */
export const NPM_MIN_VERSION = '11.17.0'

// --- CLI entry: `node scripts/publish/constants.mts --print-targets|--print-ext`
// One triple per line, for scripts/stage-npm.sh.
import { fileURLToPath } from 'node:url'

const isMainModule =
  process.argv[1] === fileURLToPath(new URL(import.meta.url))

if (isMainModule) {
  const lists: Record<string, readonly string[]> = {
    '--print-targets': TARGET_TRIPLES,
    '--print-ext': EXT_TRIPLES,
  }
  const list = lists[process.argv[2] ?? '']
  if (!list || process.argv.length !== 3) {
    console.error('usage: constants.mts --print-targets | --print-ext')
    process.exit(2)
  }
  for (const triple of list) console.log(triple)
}
