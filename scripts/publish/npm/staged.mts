/**
 * @file Staged-publish mechanics for Perry's 9 @perryts/* packages. Ported
 *   from a tiered registry-infra design, pared to Perry's fixed
 *   package set.
 *
 *   runStaged    — `npm stage publish` each package (platforms first, wrapper
 *                  last) via uploadNpmPackage. Nothing is public.
 *   verifyStagedEntry — `npm pack` locally, sha1 vs the shasum npm recorded at
 *                  stage time. Run BEFORE approve so a divergent artifact is
 *                  never promoted.
 *   runDirect    — escape hatch: classic `npm publish` (no stage/approve).
 */

import { existsSync } from 'node:fs'
import path from 'node:path'

import { ALL_PACKAGES, npmPackageDir, rootPath } from '../constants.mts'
import { logger, runCapture } from '../shared.mts'
import { readPackageJson, type StagedEntry } from './shared.mts'
import { uploadNpmPackage } from './publish-command.mts'

/** Pack one package dir with `npm pack` and return {path, sha1}. */
export async function packTarball(pkgDir: string): Promise<{
  path: string
  sha1: string
} | undefined> {
  // `npm pack --json` is deterministic about the filename + integrity; use it
  // (rather than the plain-stdout form) for a stable parse shape.
  const { stdout, code } = await runCapture('npm', ['pack', '--json'], pkgDir)
  if (code !== 0) return undefined
  let parsed: unknown
  try {
    parsed = JSON.parse(stdout)
  } catch {
    return undefined
  }
  const entry = Array.isArray(parsed) ? parsed[0] : undefined
  if (!entry || typeof entry !== 'object') return undefined
  const filename = (entry as { filename?: unknown }).filename
  const shasum = (entry as { shasum?: unknown }).shasum
  if (typeof filename !== 'string' || typeof shasum !== 'string') return undefined
  const tarball = path.join(pkgDir, filename)
  if (!existsSync(tarball)) return undefined
  return { path: tarball, sha1: shasum }
}

/**
 * Verify a staged entry: the local pack's sha1 equals the shasum npm recorded
 * when the tarball was staged. LOUD refusal on mismatch or missing shasum —
 * never approve unverified bytes.
 */
export async function verifyStagedEntry(entry: StagedEntry): Promise<boolean> {
  const { name, version, shasum: stagedShasum, stageId } = entry
  if (!stagedShasum) {
    logger.fail(
      `Pre-approve verify: no server-side shasum for ${name}@${version}.\n` +
        `  Where: npm stage list --json (stageId ${stageId}) exposed no shasum field.\n` +
        `  Fix: re-stage, or check npm version. Not approving unverified bytes.`,
    )
    return false
  }
  const pkgDir = path.join(rootPath, npmPackageDir(name))
  if (!existsSync(pkgDir)) {
    logger.fail(
      `Pre-approve verify: no package dir for ${name}@${version} at ${pkgDir}.\n` +
        `  Fix: run scripts/stage-npm.sh to materialize the npm/ package dirs first.`,
    )
    return false
  }
  const local = await packTarball(pkgDir)
  if (!local) {
    logger.fail(
      `Pre-approve verify: npm pack failed for ${name}@${version} in ${pkgDir}.\n` +
        `  Saw vs wanted: no local tarball; wanted one to hash against npm's staged shasum.`,
    )
    return false
  }
  if (local.sha1 === stagedShasum) {
    logger.info(`verify ok: ${name}@${version} (sha1 ${local.sha1})`)
    return true
  }
  logger.fail(
    `Pre-approve verify: shasum mismatch for ${name}@${version}.\n` +
      `    local pack:  ${local.sha1}\n` +
      `    npm staging: ${stagedShasum}\n` +
      `  Fix: check npm auth (npm stage download ${stageId}), or reject + re-stage. Not approving unverified bytes.`,
  )
  return false
}

/**
 * Stage all 9 packages (platforms first, wrapper last). Skips versions already
 * staged. Records failures and fails the step at the end with the full list —
 * one package's failure must not starve the packages after it (the same
 * lesson release-packages.yml's npm-publish job learned at v0.5.1151).
 */
export async function runStaged(config: {
  dryRun?: boolean
  tag?: string
}): Promise<{ staged: string[]; failed: string[] }> {
  const { dryRun = false, tag = 'latest' } = config
  const staged: string[] = []
  const failed: string[] = []
  for (const name of ALL_PACKAGES) {
    const pkgDir = path.join(rootPath, npmPackageDir(name))
    if (!existsSync(path.join(pkgDir, 'package.json'))) {
      logger.warn(`skip stage: ${name} has no package.json at ${pkgDir} (run stage-npm.sh first)`)
      failed.push(`${name} (no package.json)`)
      continue
    }
    const { version } = readPackageJson(pkgDir)
    if (!version) {
      failed.push(`${name} (no version)`)
      continue
    }
    logger.log(`=== staging ${name}@${version} ===`)
    const result = await uploadNpmPackage({ cwd: pkgDir, dryRun, mode: 'staged', tag })
    if (!result.postureOk || result.code !== 0) {
      failed.push(`${name}@${version}`)
      continue
    }
    staged.push(`${name}@${version}`)
  }
  if (failed.length > 0) logger.fail(`staging failures: ${failed.join(', ')}`)
  return { staged, failed }
}

/** `--direct` escape hatch: classic `npm publish` (upload + public in one step). */
export async function runDirect(config: {
  dryRun?: boolean
  tag?: string
}): Promise<{ published: string[]; failed: string[] }> {
  const { dryRun = false, tag = 'latest' } = config
  const published: string[] = []
  const failed: string[] = []
  for (const name of ALL_PACKAGES) {
    const pkgDir = path.join(rootPath, npmPackageDir(name))
    if (!existsSync(path.join(pkgDir, 'package.json'))) {
      failed.push(`${name} (no package.json)`)
      continue
    }
    const { version } = readPackageJson(pkgDir)
    if (!version) {
      failed.push(`${name} (no version)`)
      continue
    }
    logger.log(`=== direct publish ${name}@${version} ===`)
    const result = await uploadNpmPackage({ cwd: pkgDir, dryRun, mode: 'direct', tag })
    if (!result.postureOk || result.code !== 0) {
      failed.push(`${name}@${version}`)
      continue
    }
    published.push(`${name}@${version}`)
  }
  return { published, failed }
}
