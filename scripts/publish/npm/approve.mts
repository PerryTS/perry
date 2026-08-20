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

import { ALL_PACKAGES } from '../constants.mts'
import { logger, NON_INTERACTIVE_RENDER_ENV, runInheritTty } from '../shared.mts'
import {
  preflightSocketScanAuth,
  scanTarball,
  type ScanResult,
} from '../scan.mts'
import { fetchPublishedVersion, listStagedEntries, type StagedEntry } from './shared.mts'
import { verifyStagedEntry } from './staged.mts'

export interface ApproveOptions {
  /** TOTP code (CI / scripted). */
  otp?: string
  /** Approve without prompting; browser web-OTP drives 2FA. */
  yes?: boolean
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
): StagedEntry[] {
  const order = new Map(ALL_PACKAGES.map((n, i) => [n, i]))
  return entries
    .filter(e => order.has(e.name))
    .sort((a, b) => (order.get(a.name)! - order.get(b.name)!))
}

/** Run `npm stage approve <stageId>` for one entry, PTY-wrapped for web-OTP. */
export async function approveEntry(
  entry: StagedEntry,
  opts: ApproveOptions,
): Promise<boolean> {
  const args = ['stage', 'approve', entry.stageId]
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
  const staged = perryStagedEntries(await listStagedEntries(process.cwd()))
  if (staged.length === 0) {
    logger.fail(
      'No staged @perryts/* entries to approve. Run `npm run publish:stage` first.',
    )
    return { approved: [], failed: [], registryLive: false, scanResults: [] }
  }

  // 0. Reject a partial package set. The 9 @perryts/* packages are a fixed
  // release set (the wrapper's optionalDependencies are the platform binaries);
  // promoting a subset ships a broken install. Require every ALL_PACKAGES
  // member to be staged before any verification or approval proceeds.
  const stagedNames = new Set(staged.map(e => e.name))
  const missing = ALL_PACKAGES.filter(n => !stagedNames.has(n))
  if (missing.length > 0) {
    logger.fail(
      `Partial staged set — refusing to approve. Missing: ${missing.join(', ')}.\n` +
        `  Why: the @perryts/* packages are a fixed release set; a partial promote ships a broken install.\n` +
        `  Fix: re-run publish:stage so every package is staged, then re-approve.`,
    )
    return {
      approved: [],
      failed: staged.map(e => `${e.name}@${e.version}`),
      registryLive: false,
      scanResults: [],
    }
  }

  // 1. Verify each staged entry (sha1 gate).
  const verified: StagedEntry[] = []
  for (const entry of staged) {
    if (await verifyStagedEntry(entry)) verified.push(entry)
  }
  if (verified.length === 0) {
    logger.fail('No staged entries passed the verify gate — not approving.')
    return { approved: [], failed: staged.map(e => `${e.name}@${e.version}`), registryLive: false, scanResults: [] }
  }
  if (verified.length !== ALL_PACKAGES.length) {
    const failedVerify = staged.filter(e => !verified.includes(e))
    logger.fail(
      `Partial verify — refusing to approve. ${failedVerify.length} of ${ALL_PACKAGES.length} entries failed the sha1 gate: ` +
        `${failedVerify.map(e => `${e.name}@${e.version}`).join(', ')}.`,
    )
    return {
      approved: [],
      failed: staged.map(e => `${e.name}@${e.version}`),
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
    const res = await scanTarball(ctx, entry.name, entry.version, entry.shasum)
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

  // 3. OTP resolution (last, after every slow gate).
  if (!opts.otp && !opts.yes && !process.stdin.isTTY) {
    logger.fail(
      'approve needs an interactive terminal, or --yes / --otp.\n' +
        '  What: the promote is a 2FA action (browser web-OTP or TOTP).\n' +
        '  Fix: add --yes to let npm open the browser for web-OTP, or --otp <code>.',
    )
    return { approved: [], failed: verified.map(e => `${e.name}@${e.version}`), registryLive: false, scanResults }
  }

  // 4. npm stage approve each entry.
  const approvedEntries: StagedEntry[] = []
  const failed: string[] = []
  for (const entry of verified) {
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
      `Partial approve — ${failed.length} of ${verified.length} entries failed the promote: ${failed.join(', ')}. ` +
        'Not cutting the release — re-run publish:approve after fixing the failures.',
    )
  }

  // 5. Registry liveness — exit 0 is NOT proof.
  let registryLive = failed.length === 0
  for (const entry of approvedEntries) {
    if (!(await fetchPublishedVersion(entry.name, entry.version))) {
      logger.fail(`registry liveness: ${entry.name}@${entry.version} not resolvable after approve — do NOT cut the release.`)
      registryLive = false
    }
  }
  return { approved, failed, registryLive, scanResults }
}
