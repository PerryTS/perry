/**
 * @file THE npm upload invocation for Perry. One function builds the
 *   `npm stage publish` / `npm publish` argv, decides provenance, asserts
 *   the auth posture on both sides of the spawn, and hands back the exit code
 *   + captured output. Every path that uploads npm bytes calls it. Ported from
 *   the a tiered registry-infra design-command.mts (pnpm → npm, since
 *   Perry's root is npm-managed and npm CLI ships the same `npm stage` client).
 *
 *   `--ignore-scripts` is not optional: the tarball is already built
 *   (stage-npm.sh materialized the platform binaries) and the publish must not
 *   run lifecycle scripts. (pnpm's `--no-git-checks` is dropped — npm publish
 *   does not git-check the working tree.) `--provenance` is added only inside
 *   GitHub Actions on a PUBLIC repo — npm refuses a sigstore bundle from a
 *   private repo with E422.
 */

import { readFileSync } from 'node:fs'
import path from 'node:path'
import process from 'node:process'

import {
  publishAuthPostflight,
  publishAuthPreflight,
} from '../auth-posture.mts'
import { logger, provenanceAllowed, runInheritTee } from '../shared.mts'

export type NpmUploadMode = 'direct' | 'staged'

export interface NpmUploadResult {
  code: number
  output: string
  /** False when the auth posture refused (preflight OR postflight). */
  postureOk: boolean
  /** True when the command actually ran (false = preflight refused). */
  ran: boolean
}

/**
 * The argv for an npm upload, without the auth posture or the spawn. Pure.
 */
export function npmUploadArgs(config: {
  dryRun?: boolean
  mode?: NpmUploadMode
  provenance?: boolean
  tag?: string
}): string[] {
  const {
    dryRun = false,
    mode = 'staged',
    provenance = false,
    tag = 'latest',
  } = config
  const args = mode === 'staged' ? ['stage', 'publish'] : ['publish']
  args.push('--access', 'public', '--tag', tag, '--ignore-scripts')
  if (provenance) args.push('--provenance')
  if (dryRun) args.push('--dry-run')
  return args
}

/** Whether this run should ask npm for a provenance attestation (loud skip). */
export function resolveUploadProvenance(): boolean {
  if (process.env['GITHUB_ACTIONS'] !== 'true') return false
  if (provenanceAllowed()) return true
  logger.warn(
    'Provenance skipped: npm only verifies sigstore bundles from PUBLIC ' +
      'source repositories, and this run is not one. Provenance turns back ' +
      'on automatically when the repo is public.',
  )
  return false
}

/** The `version` of the manifest at `manifestPath`, or undefined. */
export function readPublishVersion(manifestPath: string): string | undefined {
  try {
    const parsed = JSON.parse(readFileSync(manifestPath, 'utf8')) as {
      version?: unknown
    }
    return typeof parsed.version === 'string' ? parsed.version : undefined
  } catch {
    return undefined
  }
}

/**
 * Upload one package's bytes from `cwd`, with the auth posture asserted before
 * and after. Preflight refusal → `{ code: 0, ran: false, postureOk: false }`.
 * Postflight refusal (a `Skipped OIDC` upload that exited 0) → real code with
 * `postureOk: false`. A caller that only checks `code` misses the second case.
 */
export async function uploadNpmPackage(config: {
  cwd: string
  dryRun?: boolean
  manifestPath?: string
  mode?: NpmUploadMode
  tag?: string
}): Promise<NpmUploadResult> {
  const { cwd, dryRun = false, manifestPath, mode = 'staged', tag = 'latest' } = config
  // Read the manifest version so the auth-posture gate can enforce the 0.0.0
  // reservation on direct publishes (a direct upload with a long-lived token
  // is only sanctioned for the name-reservation escape hatch).
  const version = readPublishVersion(manifestPath ?? path.join(cwd, 'package.json'))
  const reason = publishAuthPreflight({ ci: undefined, direct: mode === 'direct', dryRun, version })
  if (reason) {
    logger.fail(reason)
    return { code: 0, output: '', postureOk: false, ran: false }
  }
  const args = npmUploadArgs({
    dryRun,
    mode,
    provenance: resolveUploadProvenance(),
    tag,
  })
  // Teed: the operator watches the upload live AND the posture check below
  // reads what the registry actually said.
  const run = await runInheritTee('npm', args, cwd)
  const postureOk = !publishAuthPostflight(run.output)
  if (!postureOk) {
    logger.fail(
      'Auth posture postflight: the upload reported a failed OIDC exchange ' +
        '(Skipped OIDC / ERR_PNPM_AUTH_TOKEN_EXCHANGE). A publish that ' +
        '"succeeded" after a broken exchange is a failure — do not approve.',
    )
  }
  return { code: run.code, output: run.output, postureOk, ran: true }
}
