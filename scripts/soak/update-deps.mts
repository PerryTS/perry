#!/usr/bin/env node
/**
 * @file Soaked dependency updater — every ecosystem bumps through the same
 *   cooldown:
 *
 *   - npm: taze (maturityPeriod = SOAK_DAYS via the taze config next to the
 *     package.json) rewrites ranges, then the repo's own installer refreshes
 *     the lockfile.
 *   - cargo: `cargo update` via rustup's shim. `.cargo/config.toml` carries
 *     min-publish-age for the same window — an [unstable] cargo feature, so
 *     it bites only under a nightly toolchain; on perry's stable toolchain
 *     the automated window rides dependabot's cooldown and this manual path
 *     refuses a mutating update.
 *
 *   Usage: node scripts/soak/update-deps.mts [--npm|--cargo] [--dry-run]
 *   (no ecosystem flag = both)
 */

import { spawnSync } from 'node:child_process'
import { existsSync, realpathSync } from 'node:fs'
import path from 'node:path'
import process from 'node:process'
import { pathToFileURL } from 'node:url'

import { NPM_INSTALLERS, NPM_PKG_DIR, REPO_ROOT, RUSTUP_CARGO } from './paths.mts'

function run(cmd: string, args: string[], cwd: string): number {
  console.log(`[update-deps] ${cmd} ${args.join(' ')} (in ${path.relative(REPO_ROOT, cwd) || '.'})`)
  const res = spawnSync(cmd, args, { cwd, stdio: 'inherit' })
  if (res.error) {
    console.error(`[update-deps] ${cmd}: ${res.error.message}`)
  }
  return res.status ?? 1
}

function updateNpm(dryRun: boolean): number {
  const taze = path.join(NPM_PKG_DIR, 'node_modules/.bin/taze')
  if (!existsSync(taze)) {
    console.error(`[update-deps] taze not installed — run the installer in ${NPM_PKG_DIR} first`)
    return 1
  }
  // The taze config sets `write: true`; a dry run must override it
  // explicitly or "dry" would still rewrite package.json.
  const args = dryRun ? ['--no-write', '--no-install'] : ['--write']
  const status = run(taze, args, NPM_PKG_DIR)
  if (status !== 0 || dryRun) {
    return status
  }
  for (const [cmd, ...args] of NPM_INSTALLERS) {
    if (cmd!.includes('/') && !existsSync(cmd!)) {
      continue
    }
    return run(cmd!, args, NPM_PKG_DIR)
  }
  console.error('[update-deps] no installer found — refresh the lockfile manually')
  return 1
}

function updateCargo(dryRun: boolean): number {
  // The min-publish-age soak is an [unstable] cargo feature: only a
  // nightly honors it, and only rustup's cargo shim reads a
  // rust-toolchain.toml pin. Requiring the rustup shim keeps every
  // updater on the toolchain the repo actually pins (a Homebrew cargo
  // ignores toolchain files entirely) and makes the soak automatic the
  // day the repo moves to a dated nightly.
  if (!existsSync(RUSTUP_CARGO)) {
    console.error('[update-deps] rustup cargo shim not found — refusing a cargo that cannot follow the repo toolchain')
    return 1
  }
  // Before a mutating update, run the exact resolver in dry-run mode. Cargo
  // stable accepts the config while warning that min-publish-age is unused;
  // detecting that only after `cargo update` would be too late because the
  // lockfile may already contain fresh releases.
  if (!dryRun) {
    const preflightArgs = ['update', '--dry-run']
    console.log(`[update-deps] preflight: ${RUSTUP_CARGO} ${preflightArgs.join(' ')} (in .)`)
    const preflight = spawnSync(RUSTUP_CARGO, preflightArgs, {
      cwd: REPO_ROOT,
      encoding: 'utf8',
      stdio: ['inherit', 'inherit', 'pipe'],
    })
    const preflightStderr = preflight.stderr ?? ''
    process.stderr.write(preflightStderr)
    if (preflight.error) {
      console.error(`[update-deps] ${RUSTUP_CARGO}: ${preflight.error.message}`)
      return 1
    }
    if (isBlockedByPublishAge(preflightStderr)) {
      reportPublishAgeBlock()
      return preflight.status ?? 1
    }
    if (shouldRefuseUnsupportedCargoUpdate(dryRun, preflightStderr)) {
      console.error(
        '[update-deps] refusing mutating cargo update: this toolchain ignored\n' +
          '  [unstable] min-publish-age, so it cannot enforce the dependency soak.\n' +
          '  Use --dry-run for diagnostics or a pinned nightly that supports the key.',
      )
      return 1
    }
    if ((preflight.status ?? 1) !== 0) {
      return preflight.status ?? 1
    }
  }
  const args = dryRun ? ['update', '--dry-run'] : ['update']
  // Report honestly when the cargo-side window did not apply. perry rides
  // stable, where `[unstable] min-publish-age` is a warning-only unused
  // key — so this path is expected to say "no soak applied" today, and the
  // automated window rides dependabot's cooldown instead. It stops being a
  // silent no-op the day the repo moves to a nightly that implements it.
  console.log(`[update-deps] ${RUSTUP_CARGO} ${args.join(' ')} (in .)`)
  const res = spawnSync(RUSTUP_CARGO, args, {
    cwd: REPO_ROOT,
    encoding: 'utf8',
    stdio: ['inherit', 'inherit', 'pipe'],
  })
  const stderr = res.stderr ?? ''
  process.stderr.write(stderr)
  if (res.error) {
    console.error(`[update-deps] ${RUSTUP_CARGO}: ${res.error.message}`)
    return 1
  }
  // The window can make re-resolution IMPOSSIBLE rather than merely
  // holding a version back: if a requirement's only matching release is
  // younger than the window (e.g. `pkg = "^4"` when 4.0.0 shipped 3 days
  // ago), cargo fails the whole update. That is the soak doing its job,
  // but cargo's own help line advertises
  // CARGO_RESOLVER_INCOMPATIBLE_PUBLISH_AGE=allow — a blanket env-var
  // bypass this design deliberately does not have. Say so before someone
  // copy-pastes it out of a red terminal.
  if (isBlockedByPublishAge(stderr)) {
    reportPublishAgeBlock()
    return res.status ?? 1
  }
  if (isMinPublishAgeUnsupported(stderr)) {
    console.warn(
      '[update-deps] note: cargo ignored [unstable] min-publish-age (stable toolchain),\n' +
        '  so crate versions were NOT soak-gated here — dependabot cooldown is the\n' +
        '  enforcing surface for cargo deps in this repo.',
    )
  }
  return res.status ?? 1
}

function reportPublishAgeBlock(): void {
  console.error(
    '[update-deps] the cargo soak BLOCKED this re-resolution: a requirement can\n' +
      '  only be satisfied by a release younger than the window (see the error above).\n' +
      '  This is the window working, not a bug. Options, in order of preference:\n' +
      '    1. wait out the remaining days and re-run;\n' +
      '    2. relax/repin the requirement so an already-soaked version satisfies it;\n' +
      '    3. if the fresh release is genuinely required, adopt it as a deliberate,\n' +
      '       reviewable commit — NOT via CARGO_RESOLVER_INCOMPATIBLE_PUBLISH_AGE,\n' +
      '       which silently disables the window for every crate in the graph.',
  )
}

/**
 * cargo emits `unused config key ...` (a warning, exit 0) for an
 * `[unstable]` key it does not implement, so the ONLY signal that the soak
 * silently did not apply is this line on stderr. Exported for the tests.
 */
export function isMinPublishAgeUnsupported(stderr: string): boolean {
  return /unused config key `unstable\.min-publish-age`/.test(stderr)
}

export function shouldRefuseUnsupportedCargoUpdate(dryRun: boolean, stderr: string): boolean {
  return !dryRun && isMinPublishAgeUnsupported(stderr)
}

/**
 * cargo's resolver failure when a requirement's only candidate is inside
 * the window: `version X is too new (published N days ago, minimum age M
 * days)`. Exported for the tests; pins cargo's wording.
 */
export function isBlockedByPublishAge(stderr: string): boolean {
  return /is too new \(published .*minimum age/.test(stderr)
}

// No flag = both; naming both explicitly also means both — a naive
// "flag present = only that one" reading once made `--npm --cargo` run
// NEITHER, so this rule lives in one exported, regression-tested place.
export function selectEcosystems(argv: string[]): { npm: boolean; cargo: boolean } {
  const npmFlag = argv.includes('--npm')
  const cargoFlag = argv.includes('--cargo')
  return { npm: npmFlag || !cargoFlag, cargo: cargoFlag || !npmFlag }
}

function main(argv: string[] = process.argv.slice(2)): number {
  const dryRun = argv.includes('--dry-run')
  const { npm, cargo } = selectEcosystems(argv)
  // Run every requested ecosystem even if an earlier one fails, then
  // aggregate, so one broken ecosystem can't hide the other's drift.
  const npmStatus = npm ? updateNpm(dryRun) : 0
  const cargoStatus = cargo ? updateCargo(dryRun) : 0
  return npmStatus || cargoStatus
}

// realpath + pathToFileURL so symlinked checkouts and paths needing URL
// encoding still register as the entrypoint (ESM realpaths import.meta.url).
const isMain =
  process.argv[1] && pathToFileURL(realpathSync(process.argv[1])).href === import.meta.url
if (isMain) {
  process.exitCode = main()
}
