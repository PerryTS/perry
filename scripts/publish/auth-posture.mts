/**
 * @file Auth-posture gate, modeled on a tiered registry-infra design
 *   Perry's publish is OIDC-in-CI (the npm-stage-publish
 *   workflow, id-token: write) + browser web-OTP 2FA locally (npm stage
 *   approve). A long-lived npm token present during a publish is a DEFECT,
 *   not a convenience — it bypasses provenance and outlives the release.
 *
 *   - PREFLIGHT: refuse BEFORE any upload if a long-lived token is present in
 *     CI (where OIDC is the only sanctioned path).
 *   - POSTFLIGHT: scan command output for a failed OIDC exchange that
 *     "succeeded" anyway (ERR_PNPM_AUTH_TOKEN_EXCHANGE / Skipped OIDC) — a
 *     publish that landed after a broken exchange is treated as a failure.
 *
 *   The read-only token (PERRY_NPM_READONLY_TOKEN) is permitted everywhere —
 *   it powers registry reads (npm view, staged-download) and can never
 *   publish.
 */

import process from 'node:process'

import {
  LONG_LIVED_NPM_TOKEN_ENV_VARS,
  READONLY_TOKEN_ENV,
} from './constants.mts'

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
 * In CI, OIDC is the only sanctioned path. Locally, a direct publish is
 * permitted only for the 0.0.0 name reservation (escape hatch) — real
 * releases stage in CI and approve locally with 2FA. A direct publish of any
 * OTHER version with a long-lived token present is refused, so the escape
 * hatch cannot be widened into a general bypass.
 *
 * Pass `version` (the manifest version being published) so the reservation
 * can be enforced; without it, a direct upload with a long-lived token is
 * refused (fail-closed — the caller must prove it is the reservation).
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
      `Refusing to stage with a long-lived token present: ${present.join(', ')}. ` +
      'Staging runs in CI under OIDC; locally only the 2FA approve is sanctioned. ' +
      'Unset the token and dispatch the stage workflow instead.'
    )
  }
  // direct local publish with a long-lived token: permitted ONLY for the 0.0.0
  // name reservation escape hatch. Any other version (or an unknown version)
  // is refused — the escape hatch must not become a general bypass.
  if (version !== '0.0.0') {
    return (
      `Refusing a direct publish with a long-lived token present: ${present.join(', ')}. ` +
      `The direct escape hatch is reserved for the 0.0.0 name reservation only (got version ${version ?? '<unknown>'}). ` +
      'Real releases stage in CI under OIDC and approve locally with 2FA — unset the token and dispatch the stage workflow.'
    )
  }
  return undefined
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
 * use the staged pipeline instead.
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
    "ERROR: use `npm run publish:pipeline` (staged) — direct publish is not the sanctioned path.",
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
