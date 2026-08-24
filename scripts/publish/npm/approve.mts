/**
 * @file The promote step. Runs LOCALLY (never in CI): the stage upload used an
 *   OIDC token in CI; the
 *   approve requires human 2FA. Perry-centric: the package set is fixed (the 9
 *   @perryts/* packages), so the interactive multi-select collapses to "approve
 *   every verified + scanned staged entry".
 *
 *   Gate order (every slow gate BEFORE the OTP prompt, because TOTP is ~30s):
 *     1. list staged entries, filter to @perryts/*
 *     2. verify each (sha1 vs staged shasum) — refuse on any mismatch
 *     3. Socket full-scan each (mandatory — no flag skips this) — failed/blocked entries drop
 *     4. OTP resolution: --otp <code> | --yes (browser web-OTP) | prompt
 *     5. npm stage approve <stageId> per entry (PTY-wrapped for web-OTP)
 *     6. registry liveness: npm view <name>@<version> before minting a receipt
 */

import process from 'node:process'

import { ALL_PACKAGES, NPM_PUBLIC_REGISTRY } from '../constants.mts'
import { logger, NON_INTERACTIVE_RENDER_ENV, runInheritTty } from '../shared.mts'
import {
  preflightSocketScanAuth,
  scanTarball,
  type ScanResult,
} from '../scan.mts'
import {
  fetchPublishedShasum,
  fetchPublishedVersion,
  listStagedEntries,
  type StagedEntry,
} from './shared.mts'
import { verifyStagedEntry } from './staged.mts'

export interface ApproveOptions {
  /** The one release version this approval is allowed to promote. */
  version: string
  /** TOTP code (CI / scripted). */
  otp?: string
  /** Approve without prompting; browser web-OTP drives 2FA. */
  yes?: boolean
  /** Root containing the exact tarballs downloaded from the staging CI run. */
  proofRoot?: string
}

export interface ApproveReceipt {
  approved: string[]
  failed: string[]
  /** True only when every approved entry is resolvable on the registry. */
  registryLive: boolean
  scanResults: ScanResult[]
}

/** Filter staged entries to Perry's 9 packages, in publish order. */
export function perryStagedEntries(
  entries: readonly StagedEntry[],
  version?: string,
): StagedEntry[] {
  const order = new Map(ALL_PACKAGES.map((n, i) => [n, i]))
  return entries
    .filter(e => order.has(e.name) && (version === undefined || e.version === version))
    .sort((a, b) => (order.get(a.name)! - order.get(b.name)!))
}

/**
 * Complete a partially promoted staging set with exact-version registry
 * entries. The caller still has to verify every shasum against the CI proof.
 */
export async function completeCandidatesWithPublished(
  staged: readonly StagedEntry[],
  version: string,
  lookup: (name: string, version: string) => Promise<string | undefined> =
    fetchPublishedShasum,
): Promise<StagedEntry[]> {
  const stagedNames = new Set(staged.map(e => e.name))
  const alreadyLive: StagedEntry[] = []
  for (const name of ALL_PACKAGES) {
    if (stagedNames.has(name)) continue
    const shasum = await lookup(name, version)
    if (shasum) {
      alreadyLive.push({
        name,
        version,
        stageId: `already-live:${name}@${version}`,
        shasum,
        alreadyLive: true,
      })
    }
  }
  return perryStagedEntries([...staged, ...alreadyLive], version)
}

/** Run `npm stage approve <stageId>` for one entry, PTY-wrapped for web-OTP. */
export async function approveEntry(
  entry: StagedEntry,
  opts: ApproveOptions,
): Promise<boolean> {
  const args = [
    'stage',
    'approve',
    entry.stageId,
    '--registry',
    NPM_PUBLIC_REGISTRY,
  ]
  // Pass the OTP through NPM_CONFIG_OTP (npm's `npm_config_*` env channel)
  // rather than `--otp <code>`, so the ~30s-expiring code is not visible in
  // the process argument list. npm reads this env var at the enforced floor
  // (>= 11.17); the PTY wrapper keeps the browser web-OTP challenge alive
  // when no OTP is supplied.
  const env = {
    ...NON_INTERACTIVE_RENDER_ENV,
    ...(opts.otp ? { NPM_CONFIG_OTP: opts.otp } : {}),
  }
  const code = await runInheritTty('npm', args, process.cwd(), env)
  if (code !== 0) {
    logger.fail(`npm stage approve failed for ${entry.name}@${entry.version} (exit ${code})`)
    return false
  }
  return true
}

/**
 * The approve flow. Returns a receipt; the caller (pipeline.mts) gates the
 * release stage on `registryLive && approved.length > 0`.
 */
export async function runApprove(opts: ApproveOptions): Promise<ApproveReceipt> {
  const staged = perryStagedEntries(
    await listStagedEntries(process.cwd()),
    opts.version,
  )

  // Approval is nine sequential 2FA operations. If one fails after earlier
  // entries became public, npm removes those successful entries from staging.
  // Recover only when the immutable public dist.shasum matches this run's exact
  // CI proof; this cannot turn an old same-version artifact into a new receipt.
  const candidates = await completeCandidatesWithPublished(staged, opts.version)

  // 0. Reject a partial package set. The 9 @perryts/* packages are a fixed
  // release set (the wrapper's optionalDependencies are the platform binaries);
  // promoting a subset ships a broken install. An entry may be pending in
  // staging or already live with a registry SHA that the proof verifies.
  const candidateNames = new Set(candidates.map(e => e.name))
  const missing = ALL_PACKAGES.filter(n => !candidateNames.has(n))
  if (missing.length > 0) {
    logger.fail(
      `Partial staged/live set — refusing to approve. Missing: ${missing.join(', ')}.\n` +
        `  Why: the @perryts/* packages are a fixed release set; a partial promote ships a broken install.\n` +
        `  Fix: re-run publish:stage so every missing package is staged, then re-approve.`,
    )
    return {
      approved: [],
      failed: candidates.map(e => `${e.name}@${e.version}`),
      registryLive: false,
      scanResults: [],
    }
  }

  // 1. Verify each staged entry (sha1 gate).
  const verified: StagedEntry[] = []
  for (const entry of candidates) {
    if (await verifyStagedEntry(entry, opts.proofRoot)) verified.push(entry)
  }
  if (verified.length === 0) {
    logger.fail('No staged entries passed the verify gate — not approving.')
    return { approved: [], failed: candidates.map(e => `${e.name}@${e.version}`), registryLive: false, scanResults: [] }
  }
  if (verified.length !== ALL_PACKAGES.length) {
    const failedVerify = candidates.filter(e => !verified.includes(e))
    logger.fail(
      `Partial verify — refusing to approve. ${failedVerify.length} of ${ALL_PACKAGES.length} entries failed the sha1 gate: ` +
        `${failedVerify.map(e => `${e.name}@${e.version}`).join(', ')}.`,
    )
    return {
      approved: [],
      failed: candidates.map(e => `${e.name}@${e.version}`),
      registryLive: false,
      scanResults: [],
    }
  }

  // 2. Socket scan gate — mandatory. There is no flag or option that skips
  // this: an approve that promotes unscanned bytes to the public registry is
  // exactly the failure mode this gate exists to prevent. A missing/invalid
  // SOCKET_API_TOKEN fails closed (refuses to approve) rather than proceeding
  // without a scan.
  const scanResults: ScanResult[] = []
  const ctx = await preflightSocketScanAuth()
  if (!ctx) {
    logger.fail(
      'Socket scan auth failed — refusing to approve unscanned bytes.\n' +
        '  Fix: set SOCKET_API_TOKEN (ask a maintainer if it is not already provisioned) and re-run.',
    )
    return { approved: [], failed: verified.map(e => `${e.name}@${e.version}`), registryLive: false, scanResults: [] }
  }
  const passed: StagedEntry[] = []
  for (const entry of verified) {
    const res = await scanTarball(
      ctx,
      entry.name,
      entry.version,
      entry.shasum,
      opts.proofRoot,
    )
    scanResults.push(res)
    if (res.status === 'passed') passed.push(entry)
    else logger.fail(`scan ${res.status}: ${entry.name}@${entry.version} dropped from approve`)
  }
  if (passed.length === 0) {
    logger.fail('No staged entries passed the scan gate — not approving.')
    return { approved: [], failed: verified.map(e => `${e.name}@${e.version}`), registryLive: false, scanResults }
  }
  if (passed.length !== verified.length) {
    const dropped = verified.filter(e => !passed.includes(e))
    logger.fail(
      `Partial scan — refusing to approve. ${dropped.length} of ${verified.length} entries did not pass the scan gate: ` +
        `${dropped.map(e => `${e.name}@${e.version}`).join(', ')}.`,
    )
    return {
      approved: [],
      failed: verified.map(e => `${e.name}@${e.version}`),
      registryLive: false,
      scanResults,
    }
  }
  verified.length = 0
  verified.push(...passed)

  const pending = verified.filter(entry =>
    !entry.alreadyLive,
  )

  // 3. OTP resolution (last, after every slow gate). A fully promoted release
  // can be recovered and receipted non-interactively because no mutation or
  // 2FA operation remains; its exact public shasums were verified above.
  if (pending.length > 0 && !opts.otp && !opts.yes && !process.stdin.isTTY) {
    logger.fail(
      'approve needs an interactive terminal, or --yes / --otp.\n' +
        '  What: the promote is a 2FA action (browser web-OTP or TOTP).\n' +
        '  Fix: add --yes to let npm open the browser for web-OTP, or --otp <code>.',
    )
    return { approved: [], failed: verified.map(e => `${e.name}@${e.version}`), registryLive: false, scanResults }
  }

  // 4. npm stage approve each entry.
  const approvedEntries: StagedEntry[] = verified.filter(entry =>
    entry.alreadyLive,
  )
  const failed: string[] = []
  for (const entry of approvedEntries) {
    logger.info(`already live with matching proof: ${entry.name}@${entry.version}`)
  }
  for (const entry of pending) {
    const ok = await approveEntry(entry, opts)
    if (ok) approvedEntries.push(entry)
    else failed.push(`${entry.name}@${entry.version}`)
  }
  const approved = approvedEntries.map(e => `${e.name}@${e.version}`)

  // A partial approve (some entries failed the 2FA promote) breaks the fixed
  // 9-package release set the same way a partial verify/scan does — refuse it
  // before the caller can gate a release on the surviving subset.
  if (failed.length > 0) {
    logger.fail(
      `Partial approve — ${failed.length} of ${pending.length} pending entries failed the promote: ${failed.join(', ')}. ` +
        'Not cutting the release — re-run publish:approve after fixing the failures.',
    )
  }

  // 5. Registry liveness — exit 0 is NOT proof.
  let registryLive =
    failed.length === 0 && approvedEntries.length === ALL_PACKAGES.length
  for (const entry of approvedEntries) {
    if (!(await fetchPublishedVersion(entry.name, entry.version))) {
      logger.fail(`registry liveness: ${entry.name}@${entry.version} not resolvable after approve — do NOT cut the release.`)
      registryLive = false
    }
  }
  return { approved, failed, registryLive, scanResults }
}
