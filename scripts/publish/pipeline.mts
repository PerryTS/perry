/**
 * Perry's registry-first release orchestrator.
 *
 * npm run publish:release dispatches the exact candidate to GitHub Actions.
 * The existing Release Packages workflow builds, optionally Socket-scans, and
 * directly publishes the exact nine tarballs through npm Trusted Publisher /
 * OIDC. It verifies their public registry shasums and only then creates the tag
 * + GitHub Release. Back on the maintainer machine, this script independently
 * verifies the retained proof and final release receipt. No npm account
 * session, npm token, GitHub Environment, or local Socket token is used.
 */

import {
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  rmSync,
  writeFileSync,
} from 'node:fs'
import path from 'node:path'
import process from 'node:process'
import { fileURLToPath } from 'node:url'

import {
  CI_PACKAGE_MANIFEST,
  type CiPackageManifest,
} from './ci-package-manifest.mts'
import {
  ALL_PACKAGES,
  npmPackageDir,
  PIPELINE_STATE_DIR,
  rootPath,
} from './constants.mts'
import { checkVersionGate } from './npm/bump.mts'
import { fetchPublishedShasum } from './npm/shared.mts'
import { resolvePackageTarball } from './npm/proof.mts'
import { requireRegistryLive } from './release.mts'
import { logger, runCapture } from './shared.mts'

const PUBLISH_WORKFLOW = 'release-packages.yml'
const PROOF_ARTIFACT = 'npm-publish-package-proofs'
const PROOF_ARCHIVE = 'npm-publish-package-proofs.tar'

type SocketStatus = 'not-run' | 'passed' | 'skipped'

export interface PipelineState {
  version: string
  candidateSha?: string
  candidateRef?: string
  publishRunId?: string
  proofDir?: string
  published: string[]
  verified: string[]
  socketScan: SocketStatus
  registryLive: boolean
  released: boolean
  updatedAt: string
}

interface Candidate {
  sha: string
  ref: string
}

interface PublishWorkflowReceipt extends Candidate {
  proofDir?: string
  runId: string
}

async function releaseCoreSucceeded(runId: string): Promise<boolean> {
  const viewed = await runCapture(
    'gh',
    ['run', 'view', runId, '-R', 'PerryTS/perry', '--json', 'jobs'],
    rootPath,
  )
  if (viewed.code !== 0) return false
  try {
    const data = JSON.parse(viewed.stdout) as {
      jobs?: Array<{ conclusion?: unknown; name?: unknown }>
    }
    const jobs = data.jobs ?? []
    return ['npm-publish', 'create-release'].every(name =>
      jobs.some(job => job.name === name && job.conclusion === 'success'),
    )
  } catch {
    return false
  }
}

export function freshPublishState(
  version: string,
  receipt: PublishWorkflowReceipt,
): PipelineState {
  return {
    version,
    candidateSha: receipt.sha,
    candidateRef: receipt.ref,
    publishRunId: receipt.runId,
    proofDir: receipt.proofDir,
    published: [],
    verified: [],
    socketScan: 'not-run',
    registryLive: false,
    released: false,
    updatedAt: new Date().toISOString(),
  }
}

function statePath(version: string): string {
  return path.join(rootPath, PIPELINE_STATE_DIR, `${version}.json`)
}

function readState(version: string): PipelineState | undefined {
  const file = statePath(version)
  if (!existsSync(file)) return undefined
  try {
    const state = JSON.parse(readFileSync(file, 'utf8')) as Partial<PipelineState>
    // Ignore receipts written by the retired staged/2FA pipeline. They have a
    // different shape and must never be mistaken for direct-publish evidence.
    if (
      state.version !== version ||
      !Array.isArray(state.published) ||
      !Array.isArray(state.verified) ||
      !['not-run', 'passed', 'skipped'].includes(state.socketScan ?? '') ||
      typeof state.registryLive !== 'boolean' ||
      typeof state.released !== 'boolean' ||
      typeof state.updatedAt !== 'string'
    ) {
      return undefined
    }
    return state as PipelineState
  } catch {
    return undefined
  }
}

function writeState(state: PipelineState): void {
  const dir = path.join(rootPath, PIPELINE_STATE_DIR)
  mkdirSync(dir, { recursive: true })
  writeFileSync(statePath(state.version), `${JSON.stringify(state, null, 2)}\n`)
}

async function downloadPublishProof(runId: string): Promise<string | undefined> {
  const relativeDir = path.join(PIPELINE_STATE_DIR, 'artifacts', runId)
  const artifactDir = path.join(rootPath, relativeDir)
  rmSync(artifactDir, { recursive: true, force: true })
  mkdirSync(artifactDir, { recursive: true })
  const download = await runCapture(
    'gh',
    [
      'run', 'download', runId, '-R', 'PerryTS/perry',
      '--name', PROOF_ARTIFACT, '--dir', artifactDir,
    ],
    rootPath,
  )
  const archive = path.join(artifactDir, PROOF_ARCHIVE)
  if (download.code !== 0 || !existsSync(archive)) {
    logger.fail(`Could not download ${PROOF_ARTIFACT} from workflow run ${runId}.`)
    return undefined
  }
  const extract = await runCapture('tar', ['-xf', archive, '-C', artifactDir], rootPath)
  rmSync(archive, { force: true })
  if (extract.code !== 0) {
    logger.fail(`Could not extract the npm publication proof from run ${runId}.`)
    return undefined
  }
  const missing = ALL_PACKAGES.filter(name => {
    const dir = path.join(artifactDir, npmPackageDir(name))
    return !existsSync(dir) || readdirSync(dir).filter(file => file.endsWith('.tgz')).length !== 1
  })
  if (
    missing.length > 0 ||
    !existsSync(path.join(artifactDir, CI_PACKAGE_MANIFEST))
  ) {
    logger.fail(
      `Workflow run ${runId} did not retain the exact nine-package proof. ` +
        `Missing/ambiguous: ${missing.join(', ') || CI_PACKAGE_MANIFEST}.`,
    )
    return undefined
  }
  return relativeDir
}

/** Resolve a clean, pushed, non-main candidate branch at an exact SHA. */
async function resolveReleaseCandidate(
  config: { requireCurrentMain?: boolean } = {},
): Promise<Candidate | undefined> {
  const { requireCurrentMain = true } = config
  const status = await runCapture(
    'git',
    ['status', '--porcelain', '--untracked-files=all'],
    rootPath,
  )
  if (status.code !== 0 || status.stdout.trim()) {
    logger.fail('Release candidate is not clean — commit every candidate change before publishing.')
    return undefined
  }
  const branch = await runCapture('git', ['symbolic-ref', '--quiet', '--short', 'HEAD'], rootPath)
  const head = await runCapture('git', ['rev-parse', 'HEAD'], rootPath)
  const ref = branch.stdout.trim()
  const sha = head.stdout.trim()
  if (branch.code !== 0 || !ref || head.code !== 0 || !sha) {
    logger.fail('Release publication requires a named branch, not a detached HEAD.')
    return undefined
  }
  if (ref === 'main') {
    logger.fail('Release publication refuses moving main — use a release/vX.Y.Z candidate branch.')
    return undefined
  }
  const remote = await runCapture('git', ['ls-remote', '--heads', 'origin', `refs/heads/${ref}`], rootPath)
  const remoteSha = remote.stdout.trim().split(/\s+/)[0] ?? ''
  if (remote.code !== 0 || remoteSha !== sha) {
    logger.fail(`origin/${ref} is ${remoteSha || '<missing>'}, but local HEAD is ${sha}. Push the candidate first.`)
    return undefined
  }
  if (requireCurrentMain) {
    const main = await runCapture('git', ['ls-remote', '--heads', 'origin', 'refs/heads/main'], rootPath)
    const mainSha = main.stdout.trim().split(/\s+/)[0] ?? ''
    if (main.code !== 0 || !mainSha) {
      logger.fail('Could not resolve origin/main — refusing to publish an unverifiable candidate.')
      return undefined
    }
    const containsMain = await runCapture('git', ['merge-base', '--is-ancestor', mainSha, sha], rootPath)
    if (containsMain.code !== 0) {
      logger.fail(`Candidate ${sha} does not contain current origin/main ${mainSha}. Refresh it before publishing.`)
      return undefined
    }
  }
  return { ref, sha }
}

async function requirePinnedCandidate(state: PipelineState): Promise<Candidate | undefined> {
  const current = await resolveReleaseCandidate({ requireCurrentMain: false })
  if (!current) return undefined
  if (
    current.sha !== state.candidateSha ||
    current.ref !== state.candidateRef
  ) {
    logger.fail(
      `npm workflow run ${state.publishRunId ?? '<unknown>'} published ` +
        `${state.candidateRef}@${state.candidateSha}, but this checkout is ${current.ref}@${current.sha}.`,
    )
    return undefined
  }
  return current
}

async function dispatchPublishWorkflow(
  candidate: Candidate,
  config: { dryRun: boolean; socketScan: boolean; tag: string },
): Promise<PublishWorkflowReceipt | undefined> {
  const dispatchedAt = new Date().toISOString().replace(/\.\d{3}Z$/, 'Z')
  const args = [
    'workflow', 'run', PUBLISH_WORKFLOW, '-R', 'PerryTS/perry',
    '--ref', candidate.ref,
    '-f', `cut_release=${config.dryRun ? 'false' : 'true'}`,
    '-f', `candidate_sha=${candidate.sha}`,
    '-f', `dist_tag=${config.tag}`,
    '-f', `socket_scan=${config.socketScan ? 'true' : 'false'}`,
  ]
  const dispatched = await runCapture('gh', args, rootPath)
  if (dispatched.code !== 0) {
    logger.fail(`gh workflow run ${PUBLISH_WORKFLOW} failed (${dispatched.code}).`)
    return undefined
  }
  logger.log(`Dispatched ${PUBLISH_WORKFLOW} at ${candidate.sha}; waiting for its run id…`)
  let runId = ''
  for (let attempt = 1; attempt <= 20 && !runId; attempt += 1) {
    await new Promise(resolve => setTimeout(resolve, 3000))
    const listed = await runCapture(
      'gh',
      [
        'run', 'list', '--workflow', PUBLISH_WORKFLOW, '-R', 'PerryTS/perry',
        '--event', 'workflow_dispatch', '--limit', '20', '--json',
        'databaseId,createdAt,headBranch,headSha',
      ],
      rootPath,
    )
    try {
      const runs = JSON.parse(listed.stdout) as Array<{
        createdAt: string
        databaseId: number
        headBranch: string
        headSha: string
      }>
      const match = runs.find(run =>
        run.createdAt >= dispatchedAt &&
        run.headBranch === candidate.ref &&
        run.headSha === candidate.sha,
      )
      if (match) runId = String(match.databaseId)
    } catch {
      // Poll again; gh may have returned transient/noisy output.
    }
  }
  if (!runId) {
    logger.fail(`Could not resolve the ${PUBLISH_WORKFLOW} run id.`)
    return undefined
  }
  logger.log(`Watching release run ${runId}…`)
  const watched = await runCapture(
    'gh',
    ['run', 'watch', runId, '-R', 'PerryTS/perry', '--exit-status'],
    rootPath,
  )
  if (watched.code !== 0 && !(await releaseCoreSucceeded(runId))) {
    logger.fail(`release workflow run ${runId} failed (${watched.code}).`)
    return undefined
  }
  if (watched.code !== 0) {
    logger.warn(
      `Release core succeeded in run ${runId}; one or more post-tag distribution jobs failed. ` +
        'The release is valid, but inspect and rerun those jobs.',
    )
  }
  const proofDir = config.dryRun ? undefined : await downloadPublishProof(runId)
  if (!config.dryRun && !proofDir) return undefined
  return { ...candidate, proofDir, runId }
}

export function isCompletePublishReceipt(state: PipelineState): boolean {
  return (
    state.published.length === ALL_PACKAGES.length &&
    state.verified.length === ALL_PACKAGES.length &&
    ALL_PACKAGES.every(name =>
      state.published.includes(`${name}@${state.version}`) &&
      state.verified.includes(`${name}@${state.version}`),
    )
  )
}

async function verifyPublishedProofs(state: PipelineState): Promise<boolean> {
  if (!state.proofDir) return false
  const proofRoot = path.join(rootPath, state.proofDir)
  const manifest = JSON.parse(
    readFileSync(path.join(proofRoot, CI_PACKAGE_MANIFEST), 'utf8'),
  ) as CiPackageManifest
  if (
    manifest.version !== state.version ||
    manifest.packages.length !== ALL_PACKAGES.length
  ) {
    logger.fail('The npm proof manifest does not match the release version/package count.')
    return false
  }

  const published: string[] = []
  const verified: string[] = []
  for (const name of ALL_PACKAGES) {
    const proof = manifest.packages.find(pkg => pkg.name === name)
    if (!proof || proof.version !== state.version) continue
    const local = await resolvePackageTarball(name, proofRoot)
    if (!local || local.sha1 !== proof.sha1) {
      logger.fail(`CI proof mismatch for ${name}@${state.version}.`)
      continue
    }
    verified.push(`${name}@${state.version}`)
    const publicSha = await fetchPublishedShasum(name, state.version)
    if (publicSha !== proof.sha1) {
      logger.fail(
        `${name}@${state.version} registry sha1 is ${publicSha ?? '<not live>'}; expected CI proof ${proof.sha1}.`,
      )
      continue
    }
    published.push(`${name}@${state.version}`)
  }
  state.published = published
  state.verified = verified

  const socketReceipt = path.join(proofRoot, 'socket-scan-receipt.json')
  if (existsSync(socketReceipt)) {
    try {
      const receipt = JSON.parse(readFileSync(socketReceipt, 'utf8')) as { status?: unknown }
      if (receipt.status === 'passed' || receipt.status === 'skipped') {
        state.socketScan = receipt.status
      }
    } catch {
      // A malformed optional receipt is visible in status but does not forge a pass.
    }
  }
  state.updatedAt = new Date().toISOString()
  writeState(state)
  return isCompletePublishReceipt(state)
}

/** Verify that CI created the final tag and a published GitHub Release at the candidate SHA. */
async function verifyFinalRelease(
  version: string,
  expectedSha: string,
): Promise<boolean> {
  const tag = `v${version}`
  const remote = await runCapture(
    'git',
    ['ls-remote', 'origin', `refs/tags/${tag}`, `refs/tags/${tag}^{}`],
    rootPath,
  )
  if (remote.code !== 0) {
    logger.fail(`Could not query origin for ${tag}.`)
    return false
  }
  const refs = remote.stdout
    .trim()
    .split('\n')
    .filter(Boolean)
    .map(line => line.trim().split(/\s+/))
  const peeled = refs.find(([, ref]) => ref === `refs/tags/${tag}^{}`)?.[0]
  const direct = refs.find(([, ref]) => ref === `refs/tags/${tag}`)?.[0]
  const tagSha = peeled ?? direct
  if (tagSha !== expectedSha) {
    logger.fail(`${tag} points to ${tagSha ?? '<missing>'}; expected candidate ${expectedSha}.`)
    return false
  }

  const release = await runCapture(
    'gh',
    ['release', 'view', tag, '-R', 'PerryTS/perry', '--json', 'isDraft,url'],
    rootPath,
  )
  if (release.code !== 0) {
    logger.fail(`GitHub Release ${tag} does not exist.`)
    return false
  }
  try {
    const receipt = JSON.parse(release.stdout) as { isDraft?: unknown; url?: unknown }
    if (receipt.isDraft !== false || typeof receipt.url !== 'string') {
      logger.fail(`GitHub Release ${tag} is not published.`)
      return false
    }
    logger.log(`Verified final tag and GitHub Release: ${receipt.url}`)
    return true
  } catch {
    logger.fail(`Could not parse the GitHub Release receipt for ${tag}.`)
    return false
  }
}

function printStatus(state: PipelineState): void {
  logger.log(`=== npm-first release: v${state.version} ===`)
  logger.log(`  candidate: ${state.candidateSha ? `${state.candidateRef}@${state.candidateSha}` : '(not pinned)'}`)
  logger.log(`  workflow:  ${state.publishRunId ?? '(none)'}`)
  logger.log(`  proof:     ${state.proofDir ?? '(none)'}`)
  logger.log(`  published: ${state.published.length ? state.published.join(', ') : '(none)'}`)
  logger.log(`  verified:  ${state.verified.length ? state.verified.join(', ') : '(none)'}`)
  logger.log(`  Socket:    ${state.socketScan}`)
  logger.log(`  registry:  ${state.registryLive ? 'live' : 'not live'}`)
  logger.log(`  released:  ${state.released ? 'yes' : 'no'}`)
}

async function main(): Promise<void> {
  const argv = process.argv.slice(2)
  const known = new Set(['--publish', '--status', '--dry-run', '--socket-scan', '--tag'])
  const flags = new Set<string>()
  let tag = 'latest'
  const unknown: string[] = []
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index]!
    if (arg === '--tag') {
      const value = argv[index + 1]
      if (!value || value.startsWith('--')) unknown.push('--tag')
      else {
        tag = value
        index += 1
      }
    } else if (known.has(arg)) {
      flags.add(arg)
    } else {
      unknown.push(arg)
    }
  }
  const modes = ['--publish', '--status'].filter(mode => flags.has(mode))
  if (modes.length !== 1 || unknown.length > 0) {
    const reason = modes.length === 0
      ? 'No mode flag given.'
      : modes.length > 1
        ? `Conflicting mode flags: ${modes.join(', ')}.`
        : `Unknown flag(s): ${unknown.join(', ')}.`
    logger.fail(`${reason} Usage: publish:pipeline <--publish | --status> [--dry-run] [--socket-scan] [--tag <tag>]`)
    process.exitCode = 1
    return
  }

  const gate = await checkVersionGate(rootPath)
  const state = readState(gate.version) ?? {
    version: gate.version,
    published: [],
    verified: [],
    socketScan: 'not-run' as const,
    registryLive: false,
    released: false,
    updatedAt: new Date().toISOString(),
  }
  if (modes[0] === '--status') {
    printStatus(state)
    return
  }
  if (!gate.ok) {
    process.exitCode = 1
    return
  }

  // The candidate was cut from current main during the runbook preflight.
  // Once its exact SHA is pushed and gated, later unrelated main merges must
  // not invalidate it and force another multi-hour test cycle.
  const candidate = await resolveReleaseCandidate({ requireCurrentMain: false })
  if (!candidate) {
    process.exitCode = 1
    return
  }
  const dryRun = flags.has('--dry-run')
  const receipt = await dispatchPublishWorkflow(candidate, {
    dryRun,
    socketScan: flags.has('--socket-scan'),
    tag,
  })
  if (!receipt) {
    process.exitCode = 1
    return
  }
  if (dryRun) {
    logger.log('CI build/package dry-run succeeded; npm, tag, and GitHub Release were untouched.')
    return
  }

  const next = freshPublishState(gate.version, receipt)
  writeState(next)
  if (!(await verifyPublishedProofs(next))) {
    logger.fail('Not all public npm bytes match the retained CI proof — refusing to tag.')
    process.exitCode = 1
    return
  }
  next.registryLive = await requireRegistryLive(
    ALL_PACKAGES.map(name => ({ name, version: gate.version })),
  )
  writeState(next)
  if (!next.registryLive) {
    process.exitCode = 1
    return
  }
  const pinned = await requirePinnedCandidate(next)
  if (!pinned) {
    process.exitCode = 1
    return
  }
  next.released = await verifyFinalRelease(gate.version, pinned.sha)
  next.updatedAt = new Date().toISOString()
  writeState(next)
  printStatus(next)
  if (!next.released) process.exitCode = 1
}

if (process.argv[1] === fileURLToPath(new URL(import.meta.url))) {
  await main()
}
