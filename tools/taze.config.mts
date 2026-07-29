import { defineConfig } from 'taze'

// Cooldown derives from the canonical SOAK_DAYS so it can't drift from
// pnpm-workspace.yaml minimumReleaseAge / .npmrc min-release-age
// (scripts/soak/soak.mts asserts the data files match the same constant).
import { SOAK_DAYS } from '../scripts/soak/constants.mts'

export default defineConfig({
  interactive: false,
  loglevel: 'warn',
  maturityPeriod: SOAK_DAYS,
  mode: 'latest',
  write: true,
})
