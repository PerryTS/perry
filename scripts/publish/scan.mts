/**
 * @file Pre-approve Socket full-scan gate, modeled on a tiered registry-infra design
 *   CLI-free: everything runs through
 *   `@socketsecurity/sdk` against the Socket API directly. Each
 *   verified package tarball is submitted as a `tmp` full scan (hidden from
 *   the dashboard scan list — a promotion gate, not a tracked branch scan),
 *   and gated on the org's OWN security policy: any alert whose policy action
 *   is `error` fails the entry; `warn`-action alerts pass with counts in the
 *   receipt. Fail-closed by design.
 *
 *   Auth: `SOCKET_API_TOKEN` (the canonical env name). If absent, the gate
 *   fails with a pointer to the dashboard mint URL rather than silently
 *   passing.
 */

import { existsSync } from 'node:fs'
import path from 'node:path'
import process from 'node:process'

import { SocketSdk } from '@socketsecurity/sdk'

import {
  npmPackageDir,
  rootPath,
  SOCKET_ORG_SLUG,
  SOCKET_SCAN_REPO,
} from './constants.mts'
import { logger } from './shared.mts'
import { packTarball } from './npm/staged.mts'

export const SOCKET_TOKEN_ENV_VAR = 'SOCKET_API_TOKEN'
export const SOCKET_TOKEN_MINT_URL = 'https://socket.dev/dashboard'

export interface PolicyFailingAlert {
  artifact: string
  severity: string
  type: string
}

export interface PolicyAlertSummary {
  error: PolicyFailingAlert[]
  total: number
  warn: PolicyFailingAlert[]
}

/**
 * Pure policy evaluation: bucket every artifact alert by its org
 * security-policy action. The org's own policy decides what blocks, not a
 * hardcoded severity floor.
 */
export function summarizePolicyAlerts(
  artifacts: ReadonlyArray<{
    alerts?: ReadonlyArray<{ severity?: string; type: string }> | undefined
    name?: string
    version?: string
  }>,
  policyRules: Readonly<Record<string, { action?: string }>>,
): PolicyAlertSummary {
  const summary: PolicyAlertSummary = { error: [], total: 0, warn: [] }
  for (const artifact of artifacts) {
    for (const alert of artifact.alerts ?? []) {
      summary.total += 1
      const action = policyRules[alert.type]?.action
      if (action !== 'error' && action !== 'warn') continue
      summary[action].push({
        artifact: `${artifact.name ?? '<unnamed>'}@${artifact.version ?? '?'}`,
        severity: alert.severity ?? 'unknown',
        type: alert.type,
      })
    }
  }
  return summary
}

export interface FullScanArtifact {
  alerts?: Array<{ severity?: string; type: string }> | undefined
  name?: string
  version?: string
}

export type SecurityPolicyRules = Record<string, { action?: string }>

/** Extract the `securityPolicyRules` map from a getOrgSecurityPolicy response. */
export function extractSecurityPolicyRules(
  data: unknown,
): SecurityPolicyRules | undefined {
  if (data && typeof data === 'object') {
    const rules = (data as { securityPolicyRules?: unknown }).securityPolicyRules
    if (rules && typeof rules === 'object') {
      return rules as SecurityPolicyRules
    }
  }
  return undefined
}

/** Normalize a full-scan response (bare array or `{ artifacts: [...] }`). */
export function normalizeFullScanArtifacts(
  data: unknown,
): FullScanArtifact[] | undefined {
  if (Array.isArray(data)) return data as FullScanArtifact[]
  if (data && typeof data === 'object') {
    const arts = (data as { artifacts?: unknown }).artifacts
    if (Array.isArray(arts)) return arts as FullScanArtifact[]
  }
  return undefined
}

export interface SocketScanContext {
  orgSlug: string
  sdk: SocketSdk
}

/** Read the Socket API token from the environment. */
export function resolveSocketApiToken(
  env: NodeJS.ProcessEnv = process.env,
): string | undefined {
  const value = env[SOCKET_TOKEN_ENV_VAR]
  return value && value.length > 0 ? value : undefined
}

/**
 * Preflight auth: build an SDK from SOCKET_API_TOKEN and verify it with a
 * cheap getQuota() call. Fails closed with a mint pointer when the token is
 * missing or rejected.
 */
export async function preflightSocketScanAuth(
  orgSlug: string = SOCKET_ORG_SLUG,
): Promise<SocketScanContext | undefined> {
  const token = resolveSocketApiToken()
  if (!token) {
    logger.fail(
      `No ${SOCKET_TOKEN_ENV_VAR} in the environment — the Socket scan gate cannot run.\n` +
        `  Fix: mint a token with full-scans + report scopes at ${SOCKET_TOKEN_MINT_URL} and export ${SOCKET_TOKEN_ENV_VAR}.`,
    )
    return undefined
  }
  const sdk = new SocketSdk(token)
  try {
    const quota = await sdk.getQuota()
    if (!quota.success) {
      logger.fail(
        `Socket auth failed: getQuota() returned an unsuccessful response (status ${quota.status}) — the token was rejected.\n` +
          `  Fix: check ${SOCKET_TOKEN_ENV_VAR} at ${SOCKET_TOKEN_MINT_URL}.`,
      )
      return undefined
    }
  } catch (e) {
    logger.fail(
      `Socket auth failed: getQuota() rejected the token (${String(e)}).\n` +
        `  Fix: check ${SOCKET_TOKEN_ENV_VAR} at ${SOCKET_TOKEN_MINT_URL}.`,
    )
    return undefined
  }
  return { orgSlug, sdk }
}

export interface ScanResult {
  name: string
  version: string
  /** passed | failed | blocked (unreachable scan). */
  status: 'passed' | 'failed' | 'blocked'
  scanId?: string
  summary: PolicyAlertSummary
}

/**
 * Scan one package's tarball. Packs locally and — when the caller passes the
 * staged shasum — verifies the packed tarball's sha1 matches the staged bytes
 * BEFORE submitting, so a worktree change between verify and scan can never
 * make Socket scan different bytes from the artifact that promotion releases.
 * Submits as a tmp full scan, reads results + org security policy in parallel,
 * and buckets alerts. `error`-action alerts → failed; an unreachable/empty
 * scan → blocked (never a pass).
 */
export async function scanTarball(
  ctx: SocketScanContext,
  name: string,
  version: string,
  stagedShasum?: string,
): Promise<ScanResult> {
  const { orgSlug, sdk } = ctx
  const pkgDir = path.join(rootPath, npmPackageDir(name))
  const packed = await packTarball(pkgDir)
  if (!packed || !existsSync(packed.path)) {
    logger.fail(`scan: no local tarball for ${name}@${version} — run verify first.`)
    return { name, version, status: 'blocked', summary: { error: [], total: 0, warn: [] } }
  }
  if (stagedShasum && packed.sha1 !== stagedShasum) {
    logger.fail(
      `scan: shasum mismatch for ${name}@${version} — the local tarball no longer matches the staged bytes.\n` +
        `    local pack:  ${packed.sha1}\n` +
        `    npm staging: ${stagedShasum}\n` +
        `  Fix: the worktree changed between verify and scan; re-run publish:stage. Not scanning unverified bytes.`,
    )
    return { name, version, status: 'blocked', summary: { error: [], total: 0, warn: [] } }
  }
  const blocked = (why: string): ScanResult => {
    logger.fail(why)
    return { name, version, status: 'blocked', summary: { error: [], total: 0, warn: [] } }
  }
  let scanId: string | undefined
  try {
    const created = await sdk.createOrgFullScanFromArchive(orgSlug, packed.path, {
      repo: SOCKET_SCAN_REPO,
      tmp: true,
    })
    if (created.success) {
      scanId = (created.data as { id?: string }).id
    } else {
      return blocked(
        `archive full-scan create failed for ${name}@${version} (status ${created.status}${created.error ? `: ${String(created.error)}` : ''}).`,
      )
    }
  } catch (e) {
    return blocked(`archive full-scan create threw for ${name}@${version}: ${String(e)}`)
  }
  if (!scanId) {
    return blocked(`archive full-scan create returned no scan id for ${name}@${version}.`)
  }
  try {
    const [scan, policy] = await Promise.all([
      sdk.getFullScan(orgSlug, scanId),
      sdk.getOrgSecurityPolicy(orgSlug),
    ])
    if (!scan.success || !policy.success) {
      return blocked(`could not read full scan ${scanId} or the org security policy for ${name}@${version}.`)
    }
    const rawArtifacts = normalizeFullScanArtifacts(scan.data)
    if (!rawArtifacts || rawArtifacts.length === 0) {
      return blocked(`full scan ${scanId} returned no recognizable artifacts for ${name}@${version} — nothing was evaluated.`)
    }
    const rules = extractSecurityPolicyRules(policy.data)
    if (!rules) {
      return blocked(`the ${orgSlug} security policy was empty or unrecognized — no policy to evaluate ${name}@${version} against.`)
    }
    const summary = summarizePolicyAlerts(rawArtifacts, rules)
    logger.info(
      `full scan ${scanId} (org ${orgSlug}): ${rawArtifacts.length} artifact(s), ` +
        `${summary.total} alert(s), ${summary.error.length} error, ${summary.warn.length} warn — ${name}@${version}`,
    )
    return {
      name,
      version,
      status: summary.error.length > 0 ? 'failed' : 'passed',
      scanId,
      summary,
    }
  } catch (e) {
    return blocked(`reading full scan ${scanId} threw for ${name}@${version}: ${String(e)}`)
  }
}
