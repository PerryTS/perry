/**
 * @file Canonical soak window — the ONE source for every release-age surface.
 *   A new or bumped third-party dependency must have been published at least
 *   this long before the repo adopts it: the cooldown catches a compromised
 *   upstream before it lands. Every soak surface DERIVES from `SOAK_DAYS`
 *   instead of hand-copying the number:
 *
 *   - `.cargo/config.toml`        -> `global-min-publish-age = "<SOAK_DAYS> days"` (cargo -Zmin-publish-age)
 *   - `rust-toolchain.toml`       -> dated nightly adopted only once >= SOAK_DAYS old
 *   - `pnpm-workspace.yaml`  -> `minimumReleaseAge: <SOAK_MINUTES>` (aube reads minutes)
 *   - `.npmrc`               -> `min-release-age=<SOAK_DAYS>` (npm >= 11.10, days;
 *                                annotated excludes require npm >= 11.17)
 *   - `taze.config.mts`      -> `maturityPeriod: SOAK_DAYS` (imports this)
 *
 *   The data files can't import this module, so `scripts/soak/soak.mts`
 *   asserts they match (code-is-law parity gate).
 */

export const SOAK_DAYS = 7

// pnpm/aube `minimumReleaseAge` is expressed in MINUTES.
export const SOAK_MINUTES = SOAK_DAYS * 24 * 60

// Exclusion annotation carried on the line ABOVE every version-pinned
// `minimumReleaseAgeExclude` entry. `removable` = `published` + SOAK_DAYS;
// once `removable` is in the past the pin has soaked and must be pruned.
export const ANNOTATION_RE =
  /^#\s*published:\s*(\d{4}-\d{2}-\d{2})\s*\|\s*removable:\s*(\d{4}-\d{2}-\d{2})\s*$/

// A version-pinned exclude entry (`'name@1.2.3'` / `'@scope/name@1.2.3'`),
// as opposed to a bare name or `@scope/*` glob (which need no annotation:
// they express standing trust, not a dated soak bypass).
export const VERSION_PIN_RE = /^(@?[^@\s]+)@[^@\s]+$/

export function todayIso(): string {
  return new Date().toISOString().slice(0, 10)
}

// Shape-valid but impossible dates (2026-13-45) round-trip differently (or
// produce Invalid Date), so callers can reject them with a finding instead
// of crashing on Invalid Date arithmetic.
export function isValidIsoDate(iso: string): boolean {
  const d = new Date(`${iso}T00:00:00Z`)
  return !Number.isNaN(d.getTime()) && d.toISOString().slice(0, 10) === iso
}

export function addDaysIso(iso: string, days: number): string {
  const d = new Date(`${iso}T00:00:00Z`)
  d.setUTCDate(d.getUTCDate() + days)
  return d.toISOString().slice(0, 10)
}
