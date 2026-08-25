/**
 * @file Auth-posture gate, modeled on a tiered registry-infra design
 *   Perry's npm publication runs only in release-packages.yml with GitHub
 *   OIDC (`id-token: write`). There is no local npm login/approval path. A
 *   long-lived npm token present during a publish is a DEFECT, not a
 *   convenience — it bypasses provenance and outlives the release.
 *
 *   - PREFLIGHT: refuse BEFORE any upload if a long-lived token is present in
 *     CI (where OIDC is the only sanctioned path).
 *   - POSTFLIGHT: scan command output for a failed OIDC exchange that
 *     "succeeded" anyway (ERR_PNPM_AUTH_TOKEN_EXCHANGE / Skipped OIDC) — a
 *     publish that landed after a broken exchange is treated as a failure.
 *
 *   Public registry verification uses anonymous `npm view` reads.
 */

import process from 'node:process'

import { LONG_LIVED_NPM_TOKEN_ENV_VARS } from './constants.mts'

/** True when running inside GitHub Actions (the OIDC-sanctioned channel). */
export function isCI(): boolean {
  return process.env['GITHUB_ACTIONS'] === 'true'
}

/** The OIDC-exchange failure markers postflight scans for in command output. */
export const OIDC_FAILURE_MARKERS = [
  'ERR_PNPM_AUTH_TOKEN_EXCHANGE',
  'Skipped OIDC',
  'OIDC token exchange',
] as const

/**
 * Preflight: refuse before an upload if a long-lived publish token is present.
 * In CI, OIDC is the only sanctioned path. Local npm writes are never
 * sanctioned, including package-name reservation; provisioning a missing npm
 * package is an external organization-owner prerequisite, not a release path.
 *
 * Returns a non-empty string reason when the posture is refused, or undefined
 * when it is clean.
 */
export function publishAuthPreflight(config: {
  ci?: boolean
  direct?: boolean
  dryRun?: boolean
  version?: string
}): string | undefined {
  const { ci = isCI(), direct = false, dryRun = false, version } = config
  if (dryRun) return undefined
  const present = LONG_LIVED_NPM_TOKEN_ENV_VARS.filter(
    v => process.env[v] !== undefined && process.env[v] !== '',
  )
  if (present.length === 0) return undefined
  if (ci) {
    return (
      `Refusing to publish in CI with a long-lived token present: ${present.join(', ')}. ` +
      'CI publishes via OIDC trusted publishing (id-token: write) — unset the token and re-run.'
    )
  }
  if (!direct) {
    return (
      `Refusing to dispatch with a long-lived token present: ${present.join(', ')}. ` +
      'npm publication runs in GitHub Actions under OIDC. ' +
      'Unset the token and dispatch the release workflow instead.'
    )
  }
  return (
    `Refusing a local direct publish with a long-lived token present: ${present.join(', ')}. ` +
    `Local npm writes are not sanctioned (requested version ${version ?? '<unknown>'}); ` +
    'real releases publish in GitHub Actions under OIDC.'
  )
}

/**
 * Postflight: a publish that "succeeded" after a failed OIDC exchange is a
 * failure. Returns true when the output shows an OIDC-exchange failure marker.
 */
export function publishAuthPostflight(output: string): boolean {
  return OIDC_FAILURE_MARKERS.some(m => output.includes(m))
}

/**
 * The prepublishOnly guard entrypoint. Refuses any `npm publish` / `npm
 * publish` run that carries a long-lived token, and reminds the operator to
 * use the Actions OIDC pipeline instead.
 */
export function prepublishOnlyGuard(): number {
  const reason = publishAuthPreflight({
    ci: isCI(),
    direct: true,
    dryRun: false,
  })
  const present = LONG_LIVED_NPM_TOKEN_ENV_VARS.filter(
    v => process.env[v] !== undefined && process.env[v] !== '',
  )
  if (present.length > 0 && reason) {
    console.error(`ERROR: ${reason}`)
    return 1
  }
  console.error(
    "ERROR: use `npm run publish:release` — npm publication is only sanctioned in GitHub Actions with OIDC.",
  )
  return 1
}

// --- CLI entry (prepublishOnly guard) ---------------------------------------
import { fileURLToPath } from 'node:url'

const isMainModule =
  process.argv[1] === fileURLToPath(new URL(import.meta.url))

if (isMainModule) {
  process.exit(prepublishOnlyGuard())
}
