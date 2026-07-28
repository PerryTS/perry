/**
 * @file 1 path, 1 reference — every filesystem location the soak +
 *   external-tools scripts touch is declared here exactly once. Scripts
 *   import from this module instead of re-deriving paths, so a surface can
 *   move (or differ between repos carrying these scripts) with a one-line
 *   change. Ported from nub (nubjs/nub#442); this file is the per-repo seam.
 */

import os from 'node:os'
import path from 'node:path'
import process from 'node:process'
import { fileURLToPath } from 'node:url'

export const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..')

// Soak surfaces (repo-relative). The repo ROOT is npm-only (.npmrc +
// package-lock.json hold the parity-test fixture deps that local runs
// `npm install`); the pnpm-side soak (workspace yaml with catalog +
// minimumReleaseAge, taze) is anchored in tools/ so the workspace yaml
// never marks the npm-managed repo root as a pnpm workspace.
//
// toolchainToml is null: perry rides stable rust (CI installs
// dtolnay/rust-toolchain@stable; there is no rust-toolchain.toml). The
// dated-nightly soak check activates automatically if the repo ever pins
// one — set the path here and the surface joins the gate.
export const SURFACES: {
  cargoConfig: string
  npmrc: string
  workspaceYaml: string
  tazeConfig: string
  toolchainToml: string | null
  dependabotYml: string
} = {
  cargoConfig: '.cargo/config.toml',
  npmrc: '.npmrc',
  workspaceYaml: 'tools/pnpm-workspace.yaml',
  tazeConfig: 'tools/taze.config.mts',
  toolchainToml: null,
  dependabotYml: '.github/dependabot.yml',
}

// The directory holding the npm package the soak governs (taze runs here,
// pnpm refreshes this package's lockfile).
export const NPM_PKG_DIR = path.join(REPO_ROOT, 'tools')

// Lockfile refreshers tried in order after taze rewrites package.json.
export const NPM_INSTALLERS: string[][] = [['pnpm', 'install']]

// rustup's cargo shim — the only cargo that reads a rust-toolchain.toml and
// therefore the only one whose `cargo update` would honor the [unstable]
// min-publish-age soak (nightly-only; inert on perry's stable toolchain,
// where the automated window rides dependabot's cooldown instead).
// CARGO_HOME-aware: rustup installs its shims under $CARGO_HOME/bin.
const CARGO_HOME = process.env.CARGO_HOME || path.join(os.homedir(), '.cargo')
export const RUSTUP_CARGO = path.join(CARGO_HOME, 'bin/cargo')

// Pinned external tool manifest + the local tool rack it installs into:
// exact versions under rack/<tool>/<version>/, flat PATH handles in bin/.
export const EXTERNAL_TOOLS_JSON = path.join(REPO_ROOT, 'external-tools.json')

// CI agent image that pre-bakes the pinned toolchain + sfw (null when the
// repo has no such image — perry's Dockerfiles are product/dev images that
// build perry itself, not CI agent tooling).
export const DOCKER_PREBAKE: string | null = null

const XDG_DATA_HOME = process.env.XDG_DATA_HOME || path.join(os.homedir(), '.local/share')
export const DEV_TOOLS_DIR = path.join(XDG_DATA_HOME, 'perry/dev-tools')
export const RACK_DIR = path.join(DEV_TOOLS_DIR, 'rack')
export const BIN_DIR = path.join(DEV_TOOLS_DIR, 'bin')

// Candidates (tried in order) for installing an extracted external tool's
// runtime deps — npm first (the repo root's own package manager).
export const PM_DEP_INSTALLERS: string[][] = [
  ['npm', 'install', '--omit=dev', '--ignore-scripts', '--no-audit', '--no-fund'],
  ['pnpm', 'install', '--prod', '--ignore-scripts'],
]
