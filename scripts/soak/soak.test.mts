import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { test } from 'node:test'
import { fileURLToPath } from 'node:url'

import { SOAK_DAYS, SOAK_MINUTES, addDaysIso, todayIso } from './constants.mts'
import {
  checkCargoConfig,
  checkCatalogParity,
  checkDependabotCooldown,
  checkExcludeAnnotations,
  checkNpmrc,
  checkNpmrcExcludes,
  checkTazeConfig,
  checkToolchainSoak,
  checkWorkspaceYaml,
  fixCargoConfig,
  fixDependabotCooldown,
  fixNpmrc,
  fixWorkspaceYaml,
  main,
  parseDependabotBlocks,
  parseExcludeEntries,
  staleExcludes,
  staleNpmrcExcludes,
} from './soak.mts'

// A pin published yesterday is inside its window; one published long ago
// has expired. Built relative to today so the tests never go stale.
const FRESH_PUB = addDaysIso(todayIso(), -1)
const FRESH_REM = addDaysIso(FRESH_PUB, SOAK_DAYS)

const CLEAN_YAML = `catalog:
  taze: 19.14.1
minimumReleaseAge: ${SOAK_MINUTES}
minimumReleaseAgeExclude:
  # published: ${FRESH_PUB} | removable: ${FRESH_REM}
  - 'left-pad@1.3.0'
  - '@myorg/*'
  - react
`

test('cargo config: wrong window and missing unstable gate are findings', () => {
  const good = `[unstable]\nmin-publish-age = true\n\n[registry]\nglobal-min-publish-age = "${SOAK_DAYS} days"\n`
  assert.equal(checkCargoConfig(good, 'c').length, 0)
  assert.equal(checkCargoConfig(good.replace(`${SOAK_DAYS} days`, `${SOAK_DAYS + 1} days`), 'c').length, 1)
  assert.equal(
    checkCargoConfig(`[registry]\nglobal-min-publish-age = "${SOAK_DAYS} days"\n`, 'c').length,
    1,
  )
})

test('npmrc: window must match SOAK_DAYS and fix writes it', () => {
  assert.equal(checkNpmrc(`min-release-age=${SOAK_DAYS}\n`, 'n').length, 0)
  assert.equal(checkNpmrc('min-release-age=3\n', 'n').length, 1)
  assert.equal(checkNpmrc('# nothing\n', 'n').length, 1)
  assert.ok(fixNpmrc('# nothing\n').includes(`min-release-age=${SOAK_DAYS}`))
  assert.ok(fixNpmrc('min-release-age=3\n').includes(`min-release-age=${SOAK_DAYS}`))
})

test('npmrc excludes: version pins need dated annotations, globs do not', () => {
  // The shape a fleet repo actually uses: trusted scopes and bare names
  // are standing trust and need no annotation.
  const trusted = [
    `min-release-age=${SOAK_DAYS}`,
    'min-release-age-exclude[]=@socketsecurity/*',
    'min-release-age-exclude[]=sfw',
  ].join('\n')
  assert.deepEqual(checkNpmrcExcludes(trusted, 'n'), [])

  // A VERSION-PINNED exclude is a dated bypass — unannotated is a finding.
  const unannotated = 'min-release-age-exclude[]=lodash@4.17.21\n'
  assert.match(checkNpmrcExcludes(unannotated, 'n')[0]!.what, /lodash@4\.17\.21/)

  // Correctly annotated passes; wrong arithmetic is a finding.
  const pub = addDaysIso(todayIso(), -1)
  const ok = `# published: ${pub} | removable: ${addDaysIso(pub, SOAK_DAYS)}\nmin-release-age-exclude[]=lodash@4.17.21\n`
  assert.deepEqual(checkNpmrcExcludes(ok, 'n'), [])
  const wrongMath = `# published: ${pub} | removable: ${addDaysIso(pub, 3)}\nmin-release-age-exclude[]=lodash@4.17.21\n`
  assert.match(checkNpmrcExcludes(wrongMath, 'n')[0]!.what, /removable date/)
  const badDates = `# published: 2026-13-45 | removable: 2026-13-52\nmin-release-age-exclude[]=lodash@4.17.21\n`
  assert.match(checkNpmrcExcludes(badDates, 'n')[0]!.what, /annotation dates/)
})

test('npmrc excludes: expired version pins are warned and pruned, standing trust remains', () => {
  const published = '2020-01-01'
  const removable = addDaysIso(published, SOAK_DAYS)
  const body = [
    `min-release-age=${SOAK_DAYS}`,
    `# published: ${published} | removable: ${removable}`,
    'min-release-age-exclude[]=old@1.0.0',
    'min-release-age-exclude[]=@trusted/*',
    '',
  ].join('\n')
  assert.deepEqual(staleNpmrcExcludes(body), ['old@1.0.0'])
  const fixed = fixNpmrc(body)
  assert.ok(!fixed.includes('old@1.0.0'))
  assert.ok(!fixed.includes(published))
  assert.ok(fixed.includes('min-release-age-exclude[]=@trusted/*'))

  const fresh = `# published: ${FRESH_PUB} | removable: ${FRESH_REM}\nmin-release-age-exclude[]=fresh@1.0.0\n`
  assert.deepEqual(staleNpmrcExcludes(fresh), [])
  assert.ok(fixNpmrc(fresh).includes('fresh@1.0.0'))
})

test('workspace yaml: clean fixture passes', () => {
  assert.deepEqual(checkWorkspaceYaml(CLEAN_YAML, 'y'), [])
})

test('workspace yaml: wrong minutes value is a finding', () => {
  const bad = CLEAN_YAML.replace(String(SOAK_MINUTES), String(SOAK_MINUTES + 1))
  assert.equal(checkWorkspaceYaml(bad, 'y').filter(f => f.what.includes('minimumReleaseAge')).length, 1)
})

test('excludes: flow-style list is rejected outright', () => {
  const flow = `minimumReleaseAge: ${SOAK_MINUTES}\nminimumReleaseAgeExclude: ['left-pad@1.3.0']\n`
  const findings = checkExcludeAnnotations(flow, 'y')
  assert.equal(findings.length, 1)
  assert.match(findings[0]!.what, /flow style/)
})

test('excludes: unannotated version pin is a finding, bare/glob are not', () => {
  const yaml = 'minimumReleaseAgeExclude:\n  - lodash@4.17.21\n  - react\n  - "@myorg/*"\n'
  const findings = checkExcludeAnnotations(yaml, 'y')
  assert.equal(findings.length, 1)
  assert.match(findings[0]!.what, /lodash@4\.17\.21/)
})

test('excludes: wrong removable date is a finding; expiry is a warning, not a finding', () => {
  const wrong = `minimumReleaseAgeExclude:\n  # published: ${FRESH_PUB} | removable: ${addDaysIso(FRESH_PUB, 3)}\n  - 'a@1.0.0'\n`
  assert.match(checkExcludeAnnotations(wrong, 'y')[0]!.what, /removable date/)
  // Expired-but-valid is STALE, not unsafe: check exits clean, the stale
  // list reports it, and --fix / the soak-autofix workflow prunes it.
  const expired = `minimumReleaseAgeExclude:\n  # published: 2020-01-01 | removable: ${addDaysIso('2020-01-01', SOAK_DAYS)}\n  - 'b@1.0.0'\n`
  assert.deepEqual(checkExcludeAnnotations(expired, 'y'), [])
  assert.deepEqual(staleExcludes(expired), ['b@1.0.0'])
  // Fresh and malformed entries are never "stale".
  assert.deepEqual(staleExcludes(CLEAN_YAML), [])
  const malformed = `minimumReleaseAgeExclude:\n  # published: 2026-13-45 | removable: 2026-13-52\n  - 'c@1.0.0'\n`
  assert.deepEqual(staleExcludes(malformed), [])
})

test('excludes: impossible calendar dates are findings, not crashes', () => {
  const bad = `minimumReleaseAgeExclude:\n  # published: 2026-13-45 | removable: 2026-13-52\n  - 'c@1.0.0'\n`
  const findings = checkExcludeAnnotations(bad, 'y')
  assert.equal(findings.length, 1)
  assert.match(findings[0]!.what, /annotation dates/)
})

test('excludes: entries with trailing comments still parse', () => {
  const yaml = `minimumReleaseAgeExclude:\n  # published: ${FRESH_PUB} | removable: ${FRESH_REM}\n  - 'd@2.0.0'  # temp\n`
  assert.deepEqual(parseExcludeEntries(yaml).map(e => e.name), ['d@2.0.0'])
  assert.equal(checkExcludeAnnotations(yaml, 'y').length, 0)
})

test('fix and stale-list skip a wrong-arithmetic expired annotation', () => {
  // published + SOAK_DAYS != removable and removable is already past:
  // this must stay a check failure for a human, not silently prune —
  // the real window may still be open.
  const yaml = `minimumReleaseAge: ${SOAK_MINUTES}\nminimumReleaseAgeExclude:\n  # published: ${todayIso()} | removable: 2020-01-02\n  - 'wrongmath@1.0.0'\n`
  assert.deepEqual(staleExcludes(yaml), [])
  assert.ok(fixWorkspaceYaml(yaml).includes('wrongmath@1.0.0'))
  assert.ok(checkExcludeAnnotations(yaml, 'y').length >= 1)
})

test('fix prunes expired pins together with their annotations', () => {
  const yaml = `minimumReleaseAge: ${SOAK_MINUTES}\nminimumReleaseAgeExclude:\n  # published: 2020-01-01 | removable: ${addDaysIso('2020-01-01', SOAK_DAYS)}\n  - 'old@1.0.0'\n  # published: ${FRESH_PUB} | removable: ${FRESH_REM}\n  - 'fresh@1.0.0'\n`
  const fixed = fixWorkspaceYaml(yaml)
  assert.ok(!fixed.includes('old@1.0.0'))
  assert.ok(!fixed.includes('2020-01-01'))
  assert.ok(fixed.includes('fresh@1.0.0'))
})

test('catalog parity: exact pin must match, catalog: protocol no-ops', () => {
  const yaml = 'catalog:\n  taze: 19.14.1\n'
  const pin = (v: string) => JSON.stringify({ devDependencies: { taze: v } })
  assert.equal(checkCatalogParity(yaml, pin('19.14.1'), 'y').length, 0)
  assert.equal(checkCatalogParity(yaml, pin('19.14.2'), 'y').length, 1)
  assert.equal(checkCatalogParity(yaml, pin('catalog:'), 'y').length, 0)
})

test('catalog parity: entries after a blank line are still checked', () => {
  const yaml = 'catalog:\n  taze: 19.14.1\n\n  untracked: 1.6.4\n'
  const pkg = JSON.stringify({ devDependencies: { taze: '19.14.1', untracked: '1.0.0' } })
  assert.equal(checkCatalogParity(yaml, pkg, 'y').length, 1)
})

test('taze config: window must be imported, not hand-copied', () => {
  const good = "import { SOAK_DAYS } from './scripts/soak/constants.mts'\nexport default { maturityPeriod: SOAK_DAYS }\n"
  assert.equal(checkTazeConfig(good, 't').length, 0)
  assert.equal(checkTazeConfig('export default { maturityPeriod: 7 }\n', 't').length, 2)
  assert.equal(checkTazeConfig('export default {}\n', 't').length, 2)
  const unrelated =
    "// constants.mts and maturityPeriod are mentioned, but not wired together\nexport default { maturityPeriod: 1 }\n"
  assert.equal(checkTazeConfig(unrelated, 't').length, 2)
})

test('toolchain soak: nightly must be SOAK_DAYS old at adoption; stable passes', () => {
  const channel = '2026-07-04'
  const adopted = addDaysIso(channel, SOAK_DAYS)
  const good = `# adopted: ${adopted}\n[toolchain]\nchannel = "nightly-${channel}"\n`
  assert.equal(checkToolchainSoak(good, 't').length, 0)
  const tooFreshChannel = addDaysIso(adopted, -(SOAK_DAYS - 1))
  const tooFresh = `# adopted: ${adopted}\n[toolchain]\nchannel = "nightly-${tooFreshChannel}"\n`
  assert.match(checkToolchainSoak(tooFresh, 't')[0]!.what, /nightly soak/)
  const noDate = `[toolchain]\nchannel = "nightly-${channel}"\n`
  assert.match(checkToolchainSoak(noDate, 't')[0]!.what, /adoption date/)
  const stable = '[toolchain]\nchannel = "1.95.0"\n'
  assert.equal(checkToolchainSoak(stable, 't').length, 0)
})

test('toolchain soak: impossible calendar dates are findings, not crashes', () => {
  const bad = '# adopted: 2026-13-45\n[toolchain]\nchannel = "nightly-2026-07-04"\n'
  assert.match(checkToolchainSoak(bad, 't')[0]!.what, /soak dates/)
})

test('parser: a trailing comment on the key line still opens the block', () => {
  // Without comment tolerance, every entry under a commented key line
  // silently escaped validation — a blind spot in the bypass gate.
  const yaml = 'minimumReleaseAgeExclude:  # temporary bypasses\n  - lodash@4.17.21\n'
  assert.deepEqual(parseExcludeEntries(yaml).map(e => e.name), ['lodash@4.17.21'])
  assert.equal(checkExcludeAnnotations(yaml, 'y').length, 1)
})

test('catalog parity: malformed package.json is a finding, not a crash', () => {
  const findings = checkCatalogParity('catalog:\n  taze: 19.14.1\n', 'not json', 'y')
  assert.equal(findings.length, 1)
  assert.match(findings[0]!.what, /parse/)
})

test('parser: a column-0 line ends the exclude block', () => {
  const yaml = 'minimumReleaseAgeExclude:\n  - react\nonlyBuiltDependencies:\n  - esbuild\n'
  assert.deepEqual(parseExcludeEntries(yaml).map(e => e.name), ['react'])
})

test('parser: items at a different indent are not exclude entries', () => {
  const yaml = 'minimumReleaseAgeExclude:\n  - react\n    - not-an-entry\n  - vue\n'
  assert.deepEqual(parseExcludeEntries(yaml).map(e => e.name), ['react', 'vue'])
})

test('fix rewrites a drifted cargo window and leaves a clean one alone', () => {
  const fixed = fixCargoConfig('[registry]\nglobal-min-publish-age = "3 days"\n')
  assert.ok(fixed.includes(`"${SOAK_DAYS} days"`))
  assert.equal(fixCargoConfig(fixed), fixed)
})

const DEPENDABOT_BLOCK = (eco: string, extra = '') => `  - package-ecosystem: ${eco}
    directory: "/"
    schedule:
      interval: weekly
${extra}`

const DEPENDABOT_COOLDOWN = `    cooldown:
      default-days: ${SOAK_DAYS}
`

test('dependabot: every update block needs an explicit cooldown window', () => {
  const good = `version: 2\nupdates:\n${DEPENDABOT_BLOCK('cargo', DEPENDABOT_COOLDOWN)}${DEPENDABOT_BLOCK('github-actions', DEPENDABOT_COOLDOWN)}`
  assert.equal(checkDependabotCooldown(good, 'd').length, 0)
  // A block with no cooldown at all is drift — dependabot bumps
  // server-side, past every local soak surface.
  const missing = `version: 2\nupdates:\n${DEPENDABOT_BLOCK('cargo', DEPENDABOT_COOLDOWN)}${DEPENDABOT_BLOCK('github-actions')}`
  const findings = checkDependabotCooldown(missing, 'd')
  assert.equal(findings.length, 1)
  assert.match(findings[0]!.what, /github-actions/)
  // Wrong value is drift too.
  const drifted = good.replace(`default-days: ${SOAK_DAYS}`, 'default-days: 1')
  assert.equal(checkDependabotCooldown(drifted, 'd').length, 1)
})

test('dependabot parser: blocks split on package-ecosystem lines', () => {
  const body = `version: 2\nupdates:\n${DEPENDABOT_BLOCK('cargo')}${DEPENDABOT_BLOCK('github-actions')}`
  assert.deepEqual(
    parseDependabotBlocks(body).map(b => b.ecosystem),
    ['cargo', 'github-actions'],
  )
})

test('dependabot fix rewrites drifted values only, and is idempotent', () => {
  const drifted = `version: 2\nupdates:\n${DEPENDABOT_BLOCK('cargo', '    cooldown:\n      default-days: 1\n')}`
  const fixed = fixDependabotCooldown(drifted)
  assert.match(fixed, new RegExp(`default-days: ${SOAK_DAYS}`))
  assert.equal(fixDependabotCooldown(fixed), fixed)
  // A missing cooldown block is NOT silently inserted — that stays a
  // human edit (the check's fix line says where the two lines go).
  const missing = `version: 2\nupdates:\n${DEPENDABOT_BLOCK('cargo')}`
  assert.equal(fixDependabotCooldown(missing), missing)
})

// Glue: the tracked surfaces of THIS repo must satisfy the gate — the same
// check CI runs, exercised in-process so main() itself stays covered.
test('main --check passes against the tracked repo surfaces', () => {
  assert.equal(main([]), 0)
  assert.equal(main(['--quiet']), 0)
})

// End to end through the entrypoint guard: the CLI must resolve as main
// (realpath + file URL) and exit 0 on a clean tree.
test('CLI: node soak.mts --check --quiet exits 0', () => {
  const script = fileURLToPath(new URL('./soak.mts', import.meta.url))
  const res = spawnSync(process.execPath, [script, '--check', '--quiet'], { encoding: 'utf8' })
  assert.equal(res.status, 0, res.stderr)
})
