/**
 * @file Perry publish-pipeline orchestrator, modeled on a tiered registry-infra design
 *   Two invocations share one per-version state file at
 *   .cache/perry/publish-pipeline/<version>.json:
 *
 *     npm run publish:stage      stage-publish (dispatch CI OIDC) → verify → scan
 *     npm run publish:approve     SEPARATE explicit promote (2FA) → release
 *     npm run publish:scan        re-scan the currently-staged entries
 *     npm run publish:status      print the receipt table and exit
 *     npm run publish:release     cut the tag + GH release (after a prior approve)
 *
 *   Canonical order: stage-publish → verify → scan → [HUMAN GATE: approve] →
 *   release. Publishing never waits on a GitHub release; the release waits on
 *   the publish (a staged package is not published — staging may never be
 *   approved — so a release cut earlier can mark a version that never shipped).
 */

import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import path from 'node:path'
import process from 'node:process'

import {
  ALL_PACKAGES,
  PIPELINE_STATE_DIR,
  rootPath,
} from './constants.mts'
import { logger, runCapture, checkNpmFloor } from './shared.mts'
import { checkVersionGate } from './npm/bump.mts'
import { runStaged, verifyStagedEntry } from './npm/staged.mts'
import { listStagedEntries, type StagedEntry } from './npm/shared.mts'
import { runApprove } from './npm/approve.mts'
import {
  preflightSocketScanAuth,
  scanTarball,
  type ScanResult,
} from './scan.mts'
import { ensureTagAndRelease, requireRegistryLive } from './release.mts'
import { formatApproveGate } from './human-gate.mts'

/** The CI workflow that does the OIDC staged upload (build → stage-npm.sh → npm stage publish). */
const STAGE_WORKFLOW = 'npm-stage-publish.yml'

export interface PipelineState {
  version: string
  /** Exact pushed commit and branch used by the staging workflow. */
  candidateSha?: string
  candidateRef?: string
  stageRunId?: string
  staged: string[]
  verified: string[]
  scanResults: ScanResult[]
  /** True when the scan gate could not run at all (e.g. bad/missing SOCKET_API_TOKEN) — distinct from "ran and found nothing". */
  scanBlocked?: boolean
  approved?: string[]
  registryLive?: boolean
  released?: boolean
  updatedAt: string
}

interface Candidate {
  sha: string
  ref: string
}

interface StageWorkflowReceipt extends Candidate {
  runId: string
}

function statePath(version: string): string {
  return path.join(rootPath, PIPELINE_STATE_DIR, `${version}.json`)
}

function readState(version: string): PipelineState | undefined {
  const p = statePath(version)
  if (!existsSync(p)) return undefined
  try {
    return JSON.parse(readFileSync(p, 'utf8')) as PipelineState
  } catch {
    return undefined
  }
}

function writeState(state: PipelineState): void {
  const dir = path.join(rootPath, PIPELINE_STATE_DIR)
  if (!existsSync(dir)) mkdirSync(dir, { recursive: true })
  writeFileSync(statePath(state.version), JSON.stringify(state, null, 2) + '\n')
}

/** Resolve a clean, pushed branch tip. workflow_dispatch accepts a branch/tag, not a raw SHA. */
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
    logger.fail(
      'Release candidate is not clean — commit or remove every tracked/untracked change before staging.',
    )
    return undefined
  }
  const branch = await runCapture(
    'git',
    ['symbolic-ref', '--quiet', '--short', 'HEAD'],
    rootPath,
  )
  const head = await runCapture('git', ['rev-parse', 'HEAD'], rootPath)
  const ref = branch.stdout.trim()
  const sha = head.stdout.trim()
  if (branch.code !== 0 || !ref || head.code !== 0 || !sha) {
    logger.fail('Release staging requires a named branch, not a detached HEAD.')
    return undefined
  }
  if (ref === 'main') {
    logger.fail(
      'Release staging refuses the moving main branch — create and push a release/vX.Y.Z candidate branch.',
    )
    return undefined
  }
  const remote = await runCapture(
    'git',
    ['ls-remote', '--heads', 'origin', `refs/heads/${ref}`],
    rootPath,
  )
  const remoteSha = remote.stdout.trim().split(/\s+/)[0] ?? ''
  if (remote.code !== 0 || remoteSha !== sha) {
    logger.fail(
      `origin/${ref} is ${remoteSha || '<missing>'}, but local HEAD is ${sha}. ` +
        'Push a release-candidate branch pinned to this commit before staging.',
    )
    return undefined
  }
  if (requireCurrentMain) {
    const remoteMain = await runCapture(
      'git',
      ['ls-remote', '--heads', 'origin', 'refs/heads/main'],
      rootPath,
    )
    const mainSha = remoteMain.stdout.trim().split(/\s+/)[0] ?? ''
    if (remoteMain.code !== 0 || !mainSha) {
      logger.fail('Could not resolve origin/main — refusing to stage an unverifiable candidate.')
      return undefined
    }
    const containsMain = await runCapture(
      'git',
      ['merge-base', '--is-ancestor', mainSha, sha],
      rootPath,
    )
    if (containsMain.code !== 0) {
      logger.fail(
        `Candidate ${sha} does not contain current origin/main ${mainSha}. ` +
          'Fetch/rebase the release branch before staging.',
      )
      return undefined
    }
  }
  return { ref, sha }
}

/** Revalidate the staging receipt before approval or tag creation. */
async function requirePinnedCandidate(
  state: PipelineState,
): Promise<Candidate | undefined> {
  if (!state.candidateSha || !state.candidateRef || !state.stageRunId) {
    logger.fail(
      'The local receipt is not commit-pinned — re-run `npm run publish:stage` with the hardened pipeline.',
    )
    return undefined
  }
  // Main may advance while a staged candidate is being reviewed. Approval is
  // tied to the already-tested receipt and must not chase that moving target.
  const candidate = await resolveReleaseCandidate({ requireCurrentMain: false })
  if (!candidate) return undefined
  if (
    candidate.sha !== state.candidateSha ||
    candidate.ref !== state.candidateRef
  ) {
    logger.fail(
      `Staging run ${state.stageRunId} used ${state.candidateRef}@${state.candidateSha}, ` +
        `but this checkout is ${candidate.ref}@${candidate.sha}. Re-stage this exact candidate.`,
    )
    return undefined
  }
  return candidate
}

function isCompleteVersionSet(items: readonly string[], version: string): boolean {
  return (
    items.length === ALL_PACKAGES.length &&
    ALL_PACKAGES.every(name => items.includes(`${name}@${version}`))
  )
}

/** Dispatch the CI stage workflow and watch it to completion. */
async function dispatchStageWorkflow(
  version: string,
  candidate: Candidate,
  config: { dryRun?: boolean; tag?: string },
): Promise<StageWorkflowReceipt | undefined> {
  const { dryRun = false, tag = 'latest' } = config
  const dispatchedAt = new Date().toISOString().replace(/\.\d{3}Z$/, 'Z')
  const args = [
    'workflow',
    'run',
    STAGE_WORKFLOW,
    '-R',
    'PerryTS/perry',
    '--ref',
    candidate.ref,
    '-f',
    `publish=${dryRun ? 'false' : 'true'}`,
    '-f',
    `dist-tag=${tag}`,
    '-f',
    `candidate-sha=${candidate.sha}`,
  ]
  const run = await runCapture('gh', args, rootPath)
  if (run.code !== 0) {
    logger.fail(`gh workflow run ${STAGE_WORKFLOW} failed (${run.code}).`)
    return undefined
  }
  logger.log(`Dispatched ${STAGE_WORKFLOW} (publish=${!dryRun}, tag=${tag}). Waiting for the run to start…`)
  let runId = ''
  for (let attempt = 1; attempt <= 20 && !runId; attempt += 1) {
    await new Promise(r => setTimeout(r, 3000))
    const list = await runCapture(
      'gh',
      [
        'run', 'list', '--workflow', STAGE_WORKFLOW, '-R', 'PerryTS/perry',
        '--event', 'workflow_dispatch', '--limit', '20', '--json',
        'databaseId,createdAt,headBranch,headSha,status',
      ],
      rootPath,
    )
    try {
      const arr = JSON.parse(list.stdout) as Array<{
        databaseId: number
        createdAt: string
        headBranch: string
        headSha: string
      }>
      const match = arr.find(r =>
        r.createdAt >= dispatchedAt &&
        r.headSha === candidate.sha &&
        r.headBranch === candidate.ref,
      )
      if (match) runId = String(match.databaseId)
    } catch {
      /* poll again */
    }
  }
  if (!runId) {
    logger.fail(
      `Could not resolve the ${STAGE_WORKFLOW} run id to watch it. Check ` +
        `https://github.com/PerryTS/perry/actions/workflows/${STAGE_WORKFLOW} manually.`,
    )
    return undefined
  }
  logger.log(`Watching run ${runId}…`)
  const watch = await runCapture('gh', ['run', 'watch', runId, '-R', 'PerryTS/perry', '--exit-status'], rootPath)
  if (watch.code !== 0) {
    logger.fail(`CI stage workflow run ${runId} did not succeed (${watch.code}).`)
    return undefined
  }
  logger.log(`CI stage workflow run ${runId} succeeded — staged @perryts/* v${version}.`)
  return { ...candidate, runId }
}

/** Verify + scan the currently-staged @perryts/* entries; update state. */
async function verifyAndScan(
  version: string,
  state: PipelineState,
): Promise<PipelineState> {
  const staged = (await listStagedEntries(rootPath)).filter(
    e =>
      e.version === version &&
      ALL_PACKAGES.includes(e.name as (typeof ALL_PACKAGES)[number]),
  )
  state.staged = staged.map(e => `${e.name}@${e.version}`)
  // Surface a partial staged set immediately, not only when publish:approve
  // later refuses it — the @perryts/* packages are a fixed release set and a
  // partial stage ships a broken install if promoted.
  const stagedNames = new Set(staged.map(e => e.name))
  const missing = ALL_PACKAGES.filter(n => !stagedNames.has(n))
  if (missing.length > 0) {
    logger.fail(`Partial staged set — missing: ${missing.join(', ')}. Not all 9 @perryts/* packages are staged.`)
  }
  const verified: StagedEntry[] = []
  for (const entry of staged) {
    if (await verifyStagedEntry(entry)) verified.push(entry)
  }
  state.verified = verified.map(e => `${e.name}@${e.version}`)
  // The scan gate is mandatory — there is no flag to skip it. A missing/invalid
  // SOCKET_API_TOKEN fails closed (state.scanResults stays empty and the
  // pipeline refuses to report a clean scan) rather than silently proceeding.
  const ctx = await preflightSocketScanAuth()
  if (ctx) {
    const results: ScanResult[] = []
    for (const entry of verified) {
      results.push(await scanTarball(ctx, entry.name, entry.version, entry.shasum))
    }
    state.scanResults = results
    state.scanBlocked = false
    const failed = results.filter(r => r.status !== 'passed')
    if (failed.length > 0) {
      logger.fail(`scan gate: ${failed.length} entry/entries did not pass — fix before approve.`)
    }
  } else {
    state.scanBlocked = true
    logger.fail(
      'Socket scan auth failed — the scan did not run and cannot be skipped.\n' +
        '  Fix: set SOCKET_API_TOKEN (ask a maintainer if it is not already provisioned) and re-run.',
    )
  }
  state.updatedAt = new Date().toISOString()
  writeState(state)
  return state
}

function printStatus(state: PipelineState): void {
  logger.log(`=== publish pipeline: v${state.version} ===`)
  logger.log(
    `  candidate: ${state.candidateSha ? `${state.candidateRef}@${state.candidateSha}` : '(not pinned)'}`,
  )
  logger.log(`  stage run: ${state.stageRunId ?? '(none)'}`)
  logger.log(`  staged:    ${state.staged.length ? state.staged.join(', ') : '(none)'}`)
  logger.log(`  verified:  ${state.verified.length ? state.verified.join(', ') : '(none)'}`)
  logger.log(
    state.scanBlocked
      ? '  scan:      BLOCKED — did not run (SOCKET_API_TOKEN missing/invalid); re-run publish:scan once it is set'
      : `  scan:      ${state.scanResults.length} scanned, ${state.scanResults.filter(r => r.status !== 'passed').length} not-passed`,
  )
  logger.log(`  approved:  ${state.approved?.length ? state.approved.join(', ') : '(not approved)'}`)
  logger.log(`  registry:  ${state.registryLive ? 'live' : 'not live'}`)
  logger.log(`  released:  ${state.released ? 'yes' : 'no'}`)
}

async function main(): Promise<void> {
  // Parse argv into boolean flags + option values, so a value like `beta` (from
  // `--tag beta`) does NOT pollute the flag set and break mode routing. A value
  // that happens to equal a flag name (e.g. `--tag --approve`) can't mis-route
  // either, because `--approve` is consumed as `--tag`'s value here.
  const VALUE_OPTIONS = new Set(['--tag', '--otp'])
  // A mode flag is required — there is no implicit default mode. Running the
  // script with zero (or only --dry-run) arguments used to silently dispatch a
  // REAL staged publish; now it falls through to the usage error below instead.
  const MODE_FLAGS = new Set(['--stage-only', '--scan-only', '--approve', '--release-only', '--status'])
  const MODIFIER_FLAGS = new Set(['--dry-run', '--yes'])
  const flags = new Set<string>()
  let tag = 'latest'
  let otp: string | undefined
  for (let i = 2, { length } = process.argv; i < length; i += 1) {
    const arg = process.argv[i]!
    if (VALUE_OPTIONS.has(arg)) {
      const val = process.argv[i + 1]
      if (val !== undefined) {
        if (arg === '--tag') tag = val
        else otp = val
        i += 1
      }
    } else {
      flags.add(arg)
    }
  }
  const dryRun = flags.has('--dry-run')
  const modesGiven = [...flags].filter(f => MODE_FLAGS.has(f))
  const unknown = [...flags].filter(f => !MODE_FLAGS.has(f) && !MODIFIER_FLAGS.has(f))

  if (modesGiven.length !== 1 || unknown.length > 0) {
    logger.fail(
      (modesGiven.length === 0
        ? 'No mode flag given — refusing to guess. '
        : modesGiven.length > 1
          ? `Conflicting mode flags: ${modesGiven.join(', ')} — refusing to guess which one wins. `
          : `Unknown flag(s): ${unknown.join(', ')}. `) +
        'Usage: publish:pipeline <--stage-only | --scan-only | --approve | --release-only | --status> ' +
        '[--dry-run] [--yes] [--otp <code>] [--tag <tag>]',
    )
    process.exitCode = 1
    return
  }
  const mode = modesGiven[0]!

  // Resolve the version from the gate.
  const gate = await checkVersionGate(rootPath)
  if (!gate.ok && !flags.has('--status')) {
    process.exitCode = 1
    return
  }
  let state = readState(gate.version) ?? {
    version: gate.version,
    staged: [],
    verified: [],
    scanResults: [],
    updatedAt: new Date().toISOString(),
  }

  if (flags.has('--status')) {
    printStatus(state)
    return
  }

  // Enforce the npm floor before any stage/approve/release action (staged
  // publishing + OIDC + min-release-age all need >= NPM_MIN_VERSION).
  const floorReason = await checkNpmFloor()
  if (floorReason) {
    logger.fail(floorReason)
    process.exitCode = 1
    return
  }

  if (mode === '--stage-only') {
    const candidate = await resolveReleaseCandidate()
    if (!candidate) {
      process.exitCode = 1
      return
    }
    const receipt = await dispatchStageWorkflow(
      gate.version,
      candidate,
      { dryRun, tag },
    )
    if (!receipt) {
      process.exitCode = 1
      return
    }
    state.candidateSha = receipt.sha
    state.candidateRef = receipt.ref
    state.stageRunId = receipt.runId
    state.updatedAt = new Date().toISOString()
    writeState(state)
    if (dryRun) {
      logger.log('Dry-run build succeeded; no registry stage was created or scanned.')
      printStatus(state)
      return
    }
    state = await verifyAndScan(gate.version, state)
    printStatus(state)
    logger.log(formatApproveGate({ version: gate.version, repoPath: rootPath }))
    return
  }

  if (mode === '--scan-only') {
    state = await verifyAndScan(gate.version, state)
    printStatus(state)
    // Unlike --stage-only (where a human sees the printed status and
    // publish:approve is the real enforcement point), --scan-only is what CI
    // uses as a gate — a caller checking only the exit code must see a
    // failure for a blocked/incomplete/not-passed scan, not just a log line.
    const complete = state.staged.length === ALL_PACKAGES.length
    const allVerified = state.verified.length === state.staged.length
    const allScanned =
      !state.scanBlocked &&
      state.scanResults.length === state.verified.length &&
      state.scanResults.every(r => r.status === 'passed')
    if (!complete || !allVerified || !allScanned) {
      process.exitCode = 1
    }
    return
  }

  if (mode === '--approve') {
    const candidate = await requirePinnedCandidate(state)
    if (!candidate) {
      process.exitCode = 1
      return
    }
    const receipt = await runApprove({
      version: gate.version,
      yes: flags.has('--yes'),
      otp,
    })
    state.approved = receipt.approved
    state.registryLive = receipt.registryLive
    state.scanResults = receipt.scanResults
    state.updatedAt = new Date().toISOString()
    writeState(state)
    if (
      !receipt.registryLive ||
      !isCompleteVersionSet(receipt.approved, gate.version)
    ) {
      process.exitCode = 1
      return
    }
    // Approve continues into release (registry-liveness already confirmed in
    // runApprove, but re-gate here for the release cut).
    const live = await requireRegistryLive(
      receipt.approved.map(s => {
        const at = s.lastIndexOf('@')
        return { name: s.slice(0, at), version: s.slice(at + 1) }
      }),
    )
    if (!live) {
      process.exitCode = 1
      return
    }
    const released = await ensureTagAndRelease(gate.version, candidate.sha)
    state.released = released
    state.updatedAt = new Date().toISOString()
    writeState(state)
    if (!released) process.exitCode = 1
    return
  }

  if (mode === '--release-only') {
    const candidate = await requirePinnedCandidate(state)
    if (!candidate) {
      process.exitCode = 1
      return
    }
    if (
      !state.registryLive ||
      !isCompleteVersionSet(state.approved ?? [], gate.version)
    ) {
      logger.fail(`No complete approved+live receipt for v${gate.version} — run publish:approve first.`)
      process.exitCode = 1
      return
    }
    const packages = ALL_PACKAGES.map(name => ({ name, version: gate.version }))
    if (!(await requireRegistryLive(packages))) {
      process.exitCode = 1
      return
    }
    const released = await ensureTagAndRelease(gate.version, candidate.sha)
    state.released = released
    state.updatedAt = new Date().toISOString()
    writeState(state)
    if (!released) process.exitCode = 1
    return
  }
}

// Only run when invoked directly (node scripts/publish/pipeline.mts …), not
// when imported for testing.
import { fileURLToPath } from 'node:url'
if (process.argv[1] === fileURLToPath(new URL(import.meta.url))) {
  await main()
}
