/**
 * @file Homebrew tap publisher for Perry. Downloads the macOS arm64/x86_64
 *   release tarballs + the source archive, computes sha256 for each, renders
 *   Formula/perry.rb via brew/formula.mts, and pushes to PerryTS/homebrew-perry
 *   with HOMEBREW_TAP_TOKEN. Mirrors what the `homebrew` CI leg in
 *   release-packages.yml does, made locally runnable.
 *
 *   Usage: npm run publish:brew [-- --version 0.5.1510]
 *   Env:   HOMEBREW_TAP_TOKEN (write access to PerryTS/homebrew-perry)
 */

import { createHash } from 'node:crypto'
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { readFile } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import process from 'node:process'

import { HOMEBREW_TAP_REPO, rootPath } from '../constants.mts'
import { logger, runCapture } from '../shared.mts'
import { readCargoVersion } from '../npm/bump.mts'
import { renderPerryFormula, type PerryFormulaSpec } from './formula.mts'

/** Download a release asset to dest and return its sha256 (hex). */
async function downloadAndSha256(url: string, dest: string): Promise<string> {
  const code = await runCapture('curl', ['-fsSL', '-o', dest, url], os.tmpdir())
  if (code.code !== 0) {
    throw new Error(`download failed (${code.code}): ${url}`)
  }
  const bytes = await readFile(dest)
  return createHash('sha256').update(bytes).digest('hex')
}

async function main(): Promise<void> {
  const argv = process.argv.slice(2)
  const versionArgIdx = argv.indexOf('--version')
  const version =
    versionArgIdx !== -1 ? argv[versionArgIdx + 1] : readCargoVersion(rootPath)
  if (!version) {
    logger.fail('could not resolve version (pass --version or run from a checkout with Cargo.toml).')
    process.exitCode = 1
    return
  }
  const token = process.env['HOMEBREW_TAP_TOKEN']
  if (!token) {
    logger.fail('HOMEBREW_TAP_TOKEN is not set — cannot push to the homebrew tap.')
    process.exitCode = 1
    return
  }
  const tag = `v${version}`
  const tmp = mkdtempSync(path.join(os.tmpdir(), 'perry-brew-'))
  try {
    const base = `https://github.com/PerryTS/perry/releases/download/${tag}`
    const sourceArchive = `https://github.com/PerryTS/perry/archive/refs/tags/${tag}.tar.gz`
    logger.log(`Downloading release assets for ${tag}…`)
    const macosArm64Sha256 = await downloadAndSha256(
      `${base}/perry-macos-aarch64.tar.gz`,
      path.join(tmp, 'macos-aarch64.tar.gz'),
    )
    const macosX64Sha256 = await downloadAndSha256(
      `${base}/perry-macos-x86_64.tar.gz`,
      path.join(tmp, 'macos-x86_64.tar.gz'),
    )
    let linuxSourceSha256: string
    try {
      linuxSourceSha256 = await downloadAndSha256(sourceArchive, path.join(tmp, 'source.tar.gz'))
    } catch (e) {
      logger.fail(
        `source archive download failed (${String(e)}) — cannot build the formula without a valid Linux source sha256. ` +
          'A PLACEHOLDER sha256 would make `brew install` fail the checksum check.',
      )
      process.exitCode = 1
      return
    }
    const spec: PerryFormulaSpec = {
      version,
      macosArm64Sha256,
      macosX64Sha256,
      linuxSourceSha256,
    }
    const formula = renderPerryFormula(spec)

    // Clone the tap, write Formula/perry.rb, commit + push. The token is kept
    // out of the process argument list and out of .git/config: clone the plain
    // HTTPS URL and supply the credential through a helper that reads
    // HOMEBREW_TAP_TOKEN from the (inherited) environment.
    const credHelper = '!f() { echo "username=x-access-token"; echo "password=$HOMEBREW_TAP_TOKEN"; }; f'
    logger.log(`Cloning ${HOMEBREW_TAP_REPO}…`)
    const tapDir = path.join(tmp, 'tap')
    const clone = await runCapture(
      'git',
      ['clone', '-c', `credential.helper=${credHelper}`, `https://github.com/${HOMEBREW_TAP_REPO}.git`, tapDir],
      os.tmpdir(),
    )
    if (clone.code !== 0) {
      logger.fail(`git clone of ${HOMEBREW_TAP_REPO} failed (${clone.code}).`)
      process.exitCode = 1
      return
    }
    const formulaDir = path.join(tapDir, 'Formula')
    mkdirSync(formulaDir, { recursive: true })
    writeFileSync(path.join(formulaDir, 'perry.rb'), formula)

    const commitMsg = `perry ${tag}`
    // A re-run for a version already in the tap leaves the formula unchanged;
    // `git commit` exits non-zero then. Treat "nothing to commit" as success
    // (the tap already carries the correct formula) instead of failing.
    const addR = await runCapture('git', ['add', path.join('Formula', 'perry.rb')], tapDir)
    if (addR.code !== 0) {
      logger.fail(`git add failed (${addR.code}).`)
      process.exitCode = 1
      return
    }
    const diffR = await runCapture('git', ['diff', '--cached', '--quiet'], tapDir)
    if (diffR.code === 0) {
      logger.log(`Formula unchanged for ${tag} — tap already carries it. Nothing to push.`)
    } else {
      for (const [args, label] of [
        [['commit', '-m', commitMsg], 'git commit'],
        [['-c', `credential.helper=${credHelper}`, 'push', 'origin', 'HEAD'], 'git push'],
      ] as [string[], string][]) {
        const r = await runCapture('git', args, tapDir)
        if (r.code !== 0) {
          logger.fail(`${label} failed (${r.code}).`)
          process.exitCode = 1
          return
        }
      }
      logger.log(`Homebrew tap updated: ${HOMEBREW_TAP_REPO}@${tag}. brew install perryts/perry/perry`)
    }
  } finally {
    rmSync(tmp, { recursive: true, force: true })
  }
}

// Only run when invoked directly (node scripts/publish/brew/tap-publish.mts),
// not when imported for testing.
import { fileURLToPath } from 'node:url'
if (process.argv[1] === fileURLToPath(new URL(import.meta.url))) {
  await main()
}
