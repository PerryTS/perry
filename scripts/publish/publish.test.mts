/**
 * @file Unit tests for the pure publish-pipeline surfaces. Run with
 *   `node --test scripts/publish/publish.test.mts`. Covers the brew formula
 *   renderer, the Socket policy-alert bucketing, the human-gate block shape,
 *   and the auth-posture refusal of long-lived tokens (sabotage-tested: the
 *   refusal is asserted with a token present, and the clean path with it
 *   absent — both halves of the gate).
 */

import { test } from 'node:test'
import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import {
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import process from 'node:process'
import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'

import { renderPerryFormula } from './brew/formula.mts'
import { summarizePolicyAlerts, normalizeFullScanArtifacts } from './scan.mts'
import { formatHumanGate } from './human-gate.mts'
import { publishAuthPreflight } from './auth-posture.mts'
import { compareSemver, extractFirstJson } from './shared.mts'
import { parseStageListJson } from './npm/shared.mts'
import { perryStagedEntries } from './npm/approve.mts'
import { tagExists } from './npm/bump.mts'
import { verifyStagedEntry } from './npm/staged.mts'
import { freshStageState, isCompleteScanReceipt } from './pipeline.mts'
import {
  INLINE_RELEASE_NOTES_MAX_BYTES,
  planReleaseNotes,
} from './release.mts'
import { ALL_PACKAGES, NPM_MIN_VERSION } from './constants.mts'

const PUBLISH_DIR = path.dirname(fileURLToPath(import.meta.url))

test('renderPerryFormula: macos arm64/x86_64 binaries + linux source build', () => {
  const f = renderPerryFormula({
    version: '0.5.1510',
    macosArm64Sha256: 'a'.repeat(64),
    macosX64Sha256: 'b'.repeat(64),
    linuxSourceSha256: 'c'.repeat(64),
  })
  assert.match(f, /class Perry < Formula/)
  assert.match(f, /version "0\.5\.1510"/)
  assert.match(f, /on_macos do/)
  assert.match(f, /on_arm do/)
  assert.match(f, /perry-macos-aarch64\.tar\.gz/)
  assert.match(f, /sha256 "a{64}"/)
  assert.match(f, /on_intel do/)
  assert.match(f, /perry-macos-x86_64\.tar\.gz/)
  assert.match(f, /sha256 "b{64}"/)
  assert.match(f, /on_linux do/)
  assert.match(f, /archive\/refs\/tags\/v0\.5\.1510\.tar\.gz/)
  assert.match(f, /sha256 "c{64}"/)
  assert.match(f, /depends_on "rust" => :build/)
  assert.match(f, /def install/)
  assert.match(f, /test do/)
})

test('summarizePolicyAlerts: error-action blocks, warn-action passes, unknown ignored', () => {
  const artifacts = [
    {
      name: '@perryts/perry-darwin-arm64',
      version: '0.5.1510',
      alerts: [
        { type: 'npmMalware', severity: 'critical' }, // error
        { type: 'npmDeprecated', severity: 'low' }, // warn
        { type: 'someOther', severity: 'high' }, // no policy entry → ignored
      ],
    },
  ]
  const rules = {
    npmMalware: { action: 'error' },
    npmDeprecated: { action: 'warn' },
  }
  const s = summarizePolicyAlerts(artifacts, rules)
  assert.equal(s.total, 3)
  assert.equal(s.error.length, 1)
  assert.equal(s.error[0]!.type, 'npmMalware')
  assert.equal(s.warn.length, 1)
  assert.equal(s.warn[0]!.type, 'npmDeprecated')
})

test('normalizeFullScanArtifacts: bare array and {artifacts: [...]} shapes', () => {
  assert.deepEqual(normalizeFullScanArtifacts([{ name: 'a' }]), [{ name: 'a' }])
  assert.deepEqual(
    normalizeFullScanArtifacts({ artifacts: [{ name: 'b' }] }),
    [{ name: 'b' }],
  )
  assert.equal(normalizeFullScanArtifacts({ weird: 1 }), undefined)
  assert.equal(normalizeFullScanArtifacts(undefined), undefined)
})

test('extractFirstJson: balanced object, array, and noisy-wrapped array', () => {
  // Object — the original supported shape.
  assert.equal(extractFirstJson('{"a":1}'), '{"a":1}')
  // Array — npm stage list --json emits an array of entries.
  assert.equal(extractFirstJson('[{"id":"a"},{"id":"b"}]'), '[{"id":"a"},{"id":"b"}]')
  // Noisy stdout wrapping a JSON array (progress lines before the array).
  const noisy = '⠹ downloading...\n[{"id":"a","packageName":"@perryts/perry","version":"1.0.0"}]\n'
  assert.equal(extractFirstJson(noisy), '[{"id":"a","packageName":"@perryts/perry","version":"1.0.0"}]')
  // Strings containing braces/brackets must not break the balance.
  assert.equal(extractFirstJson('[{"a":"}]"},{"b":"["}]'), '[{"a":"}]"},{"b":"["}]')
  assert.equal(extractFirstJson('no json here'), undefined)
})

test('parseStageListJson: array of entries parses to staged entries', () => {
  const text = '[{"id":"stage-1","packageName":"@perryts/perry","version":"0.5.1","shasum":"abc"}]'
  const entries = parseStageListJson(text)
  assert.equal(entries.length, 1)
  assert.equal(entries[0]!.name, '@perryts/perry')
  assert.equal(entries[0]!.stageId, 'stage-1')
  assert.equal(entries[0]!.version, '0.5.1')
  assert.equal(entries[0]!.shasum, 'abc')
  // Noisy wrapper around the array must not drop entries.
  const noisy = 'progress...\n[{"id":"s2","name":"@perryts/perry-darwin-arm64","version":"0.5.1"}]'
  const entries2 = parseStageListJson(noisy)
  assert.equal(entries2.length, 1)
  assert.equal(entries2[0]!.name, '@perryts/perry-darwin-arm64')
})

test('perryStagedEntries: approval is restricted to the candidate version', () => {
  const entries = [
    {
      name: '@perryts/perry-darwin-arm64',
      version: '0.5.1519',
      stageId: 'candidate',
    },
    {
      name: '@perryts/perry-darwin-arm64',
      version: '0.5.1518',
      stageId: 'stale',
    },
    {
      name: '@somewhere/else',
      version: '0.5.1519',
      stageId: 'foreign',
    },
  ]
  assert.deepEqual(
    perryStagedEntries(entries, '0.5.1519').map(e => e.stageId),
    ['candidate'],
  )
})

test('tagExists: inability to query origin fails closed', async () => {
  const cwd = mkdtempSync(path.join(os.tmpdir(), 'perry-tag-gate-'))
  try {
    const init = spawnSync('git', ['init', '--quiet'], { cwd, encoding: 'utf8' })
    assert.equal(init.status, 0)
    await assert.rejects(
      tagExists('0.5.1519', cwd),
      /could not query origin for refs\/tags\/v0\.5\.1519/,
    )
  } finally {
    rmSync(cwd, { recursive: true, force: true })
  }
})

test('freshStageState: a new stage cannot inherit an old approval receipt', () => {
  const state = freshStageState('0.5.1519', {
    sha: 'a'.repeat(40),
    ref: 'release/v0.5.1519',
    runId: '12345',
    stageProofDir: '.cache/perry/publish-pipeline/artifacts/12345',
  })
  assert.deepEqual(state.staged, [])
  assert.deepEqual(state.verified, [])
  assert.deepEqual(state.scanResults, [])
  assert.deepEqual(state.approved, [])
  assert.equal(state.registryLive, false)
  assert.equal(state.released, false)
  assert.equal(state.candidateSha, 'a'.repeat(40))
  assert.equal(state.candidateRef, 'release/v0.5.1519')
  assert.equal(state.stageRunId, '12345')
  assert.equal(
    state.stageProofDir,
    '.cache/perry/publish-pipeline/artifacts/12345',
  )
})

test('verifyStagedEntry: local approval verifies the exact CI proof tarball', async () => {
  const proofRoot = mkdtempSync(path.join(os.tmpdir(), 'perry-stage-proof-'))
  try {
    const packageDir = path.join(proofRoot, 'npm/perry-win32-arm64')
    mkdirSync(packageDir, { recursive: true })
    const tarball = path.join(packageDir, 'perryts-perry-win32-arm64-0.5.1519.tgz')
    writeFileSync(tarball, 'exact staged bytes')
    const shasum = createHash('sha1')
      .update(readFileSync(tarball))
      .digest('hex')
    const entry = {
      name: '@perryts/perry-win32-arm64',
      version: '0.5.1519',
      stageId: 'stage-proof',
      shasum,
    }
    assert.equal(await verifyStagedEntry(entry, proofRoot), true)
    assert.equal(
      await verifyStagedEntry({ ...entry, shasum: '0'.repeat(40) }, proofRoot),
      false,
    )
  } finally {
    rmSync(proofRoot, { recursive: true, force: true })
  }
})

test('isCompleteScanReceipt: every exact package/version must pass', () => {
  const version = '0.5.1519'
  const packages = [...ALL_PACKAGES]
  const state = {
    version,
    staged: packages.map(name => `${name}@${version}`),
    verified: packages.map(name => `${name}@${version}`),
    scanResults: packages.map(name => ({
      name,
      version,
      status: 'passed' as const,
      summary: { error: [], total: 0, warn: [] },
    })),
    updatedAt: new Date().toISOString(),
  }
  assert.equal(isCompleteScanReceipt(state), true)
  state.scanResults[0] = { ...state.scanResults[0]!, status: 'blocked' }
  assert.equal(isCompleteScanReceipt(state), false)
})

test('formatHumanGate: the 🖐 block shape with both lanes', () => {
  const block = formatHumanGate({
    name: 'approve',
    index: '1/1',
    need: 'the staged upload is verified + scanned; 2FA promote is human.',
    mind: 'npm stage approve requires browser web-OTP 2FA.',
    you: 'npm run publish:approve',
    me: 'I will run publish:approve so npm opens the browser for web-OTP.',
    then: 'registry liveness + tag + immutable GitHub release.',
  })
  assert.match(block, /^🖐  HUMAN GATE — approve \[1\/1\]/)
  assert.match(block, /A\) You: npm run publish:approve/)
  assert.match(block, /B\) Me: I will run/)
  assert.match(block, /Then: registry liveness/)
})

// Save + clear every long-lived token env var the gate checks, and restore
// them after. Tests that only clear NPM_TOKEN break on a runner that defines
// NODE_AUTH_TOKEN or NPM_AUTH_TOKEN — the gate checks all three.
const TOKEN_VARS = ['NPM_TOKEN', 'NODE_AUTH_TOKEN', 'NPM_AUTH_TOKEN'] as const

function snapshotTokenVars(): Record<string, string | undefined> {
  const snap: Record<string, string | undefined> = {}
  for (const v of TOKEN_VARS) snap[v] = process.env[v]
  return snap
}

function clearTokenVars(): void {
  for (const v of TOKEN_VARS) delete process.env[v]
}

function restoreTokenVars(snap: Record<string, string | undefined>): void {
  for (const v of TOKEN_VARS) {
    if (snap[v] === undefined) delete process.env[v]
    else process.env[v] = snap[v]
  }
}

test('publishAuthPreflight: refuses each long-lived token in CI (sabotage)', () => {
  for (const v of TOKEN_VARS) {
    const snap = snapshotTokenVars()
    clearTokenVars()
    process.env[v] = 'npm_secret_live_token'
    try {
      const reason = publishAuthPreflight({ ci: true, direct: false, dryRun: false })
      assert.ok(reason, `must refuse when ${v} is present in CI`)
      assert.match(reason!, /Refusing to publish in CI with a long-lived token/)
    } finally {
      restoreTokenVars(snap)
    }
  }
})

test('publishAuthPreflight: clean path with no long-lived token', () => {
  const snap = snapshotTokenVars()
  clearTokenVars()
  try {
    const reason = publishAuthPreflight({ ci: true, direct: false, dryRun: false })
    assert.equal(reason, undefined, 'must not refuse when no long-lived token is present')
  } finally {
    restoreTokenVars(snap)
  }
})

test('publishAuthPreflight: refuses a direct publish with a token for non-0.0.0', () => {
  const snap = snapshotTokenVars()
  clearTokenVars()
  process.env['NPM_TOKEN'] = 'npm_secret_live_token'
  try {
    const reason = publishAuthPreflight({ ci: false, direct: true, dryRun: false, version: '1.2.3' })
    assert.ok(reason, 'must refuse a direct publish of a real version with a long-lived token')
    assert.match(reason!, /0\.0\.0 name reservation/)
  } finally {
    restoreTokenVars(snap)
  }
})

test('publishAuthPreflight: allows the 0.0.0 reservation direct publish with a token', () => {
  const snap = snapshotTokenVars()
  clearTokenVars()
  process.env['NPM_TOKEN'] = 'npm_secret_live_token'
  try {
    const reason = publishAuthPreflight({ ci: false, direct: true, dryRun: false, version: '0.0.0' })
    assert.equal(reason, undefined, 'the 0.0.0 reservation escape hatch must pass the posture gate')
  } finally {
    restoreTokenVars(snap)
  }
})

test('publishAuthPreflight: dry-run never refuses', () => {
  const snap = snapshotTokenVars()
  clearTokenVars()
  process.env['NPM_TOKEN'] = 'npm_secret_live_token'
  try {
    const reason = publishAuthPreflight({ ci: true, direct: false, dryRun: true })
    assert.equal(reason, undefined, 'dry-run must skip the posture gate')
  } finally {
    restoreTokenVars(snap)
  }
})

test('compareSemver: major/minor/patch ordering', () => {
  assert.equal(compareSemver('11.17.0', '11.17.0'), 0)
  assert.ok(compareSemver('11.17.0', '11.5.1') > 0)
  assert.ok(compareSemver('11.5.1', '11.17.0') < 0)
  assert.ok(compareSemver('12.0.0', '11.99.99') > 0)
  assert.ok(compareSemver('11.15.0', '11.17.0') < 0)
})

// Regression coverage for the incident class this repo has already been
// bitten by once: a bare `import` of an entry-point module firing a REAL
// side-effecting command (a real `cargo publish` fired from an import-test
// with no isMainModule guard). Static, not a live import — asserting this by
// actually importing these modules would itself risk running `main()` again
// if a future edit ever breaks the guard, which is exactly the bug this test
// exists to catch before it can do that.
const MAIN_GUARDED_ENTRYPOINTS = [
  'pipeline.mts',
  'auth-posture.mts',
  'brew/tap-publish.mts',
  'cargo/ffi-publish.mts',
] as const

function assertMainGuardIsTrailing(relPath: string): void {
  const src = readFileSync(path.join(PUBLISH_DIR, relPath), 'utf8')
  const guardRe = /if\s*\(\s*(?:isMainModule|process\.argv\[1\]\s*===\s*fileURLToPath\(new URL\(import\.meta\.url\)\))\s*\)\s*\{/
  const m = guardRe.exec(src)
  assert.ok(m, `${relPath}: missing the isMainModule guard around its entry call`)
  // Walk brace depth from the guard's opening `{` to find its matching close,
  // then require everything after that close to be blank/whitespace — i.e.
  // the guard is the LAST top-level statement, so nothing side-effecting can
  // be reintroduced below it.
  let depth = 0
  let i = m.index + m[0].length - 1
  for (; i < src.length; i += 1) {
    if (src[i] === '{') depth += 1
    else if (src[i] === '}') {
      depth -= 1
      if (depth === 0) break
    }
  }
  assert.ok(i < src.length, `${relPath}: unbalanced braces while scanning the guard block`)
  const trailing = src.slice(i + 1)
  assert.equal(
    trailing.trim(),
    '',
    `${relPath}: code follows the isMainModule guard — a bare import would run it as a side effect`,
  )
}

for (const entry of MAIN_GUARDED_ENTRYPOINTS) {
  test(`${entry}: main() only runs behind the isMainModule guard, which is the last statement`, () => {
    assertMainGuardIsTrailing(entry)
  })
}

test('pipeline.mts: no mode flag is a usage error, not a default action', () => {
  const src = readFileSync(path.join(PUBLISH_DIR, 'pipeline.mts'), 'utf8')
  // The zero-flag footgun this guards against: running the script with no
  // arguments used to silently dispatch a REAL staged publish (flags.size
  // === 0 fell into the --stage-only branch). There must be no code path
  // that treats an empty/absent mode as --stage-only.
  assert.doesNotMatch(
    src,
    /flags\.size\s*===\s*0/,
    'pipeline.mts must not special-case zero flags into a default mode',
  )
  assert.match(
    src,
    /No mode flag given/,
    'pipeline.mts must refuse to run without an explicit mode flag',
  )
})

test('pipeline.mts: --scan-only sets a failing exit code on an incomplete/blocked/not-passed scan', () => {
  // --scan-only is what CI's "Socket scan the staged tarballs" step relies on
  // as a gate (npm-stage-publish.yml) — unlike --stage-only, where a human
  // reads the printed status and publish:approve is the real enforcement
  // point, a CI caller only sees the exit code. Assert the branch actually
  // sets process.exitCode = 1 rather than always returning 0.
  const src = readFileSync(path.join(PUBLISH_DIR, 'pipeline.mts'), 'utf8')
  const scanOnlyBranch = src.slice(src.indexOf("mode === '--scan-only'"))
  assert.match(
    scanOnlyBranch,
    /process\.exitCode = 1/,
    '--scan-only must set a non-zero exit code when the scan did not fully pass',
  )
})

test('pipeline.mts: stage dispatch and release receipt are pinned to one commit', () => {
  const src = readFileSync(path.join(PUBLISH_DIR, 'pipeline.mts'), 'utf8')
  const workflow = readFileSync(
    path.join(PUBLISH_DIR, '../../.github/workflows/npm-stage-publish.yml'),
    'utf8',
  )
  assert.match(src, /'--ref',\s*candidate\.ref/)
  assert.match(src, /`candidate-sha=\$\{candidate\.sha\}`/)
  assert.match(src, /r\.headSha === candidate\.sha/)
  assert.match(src, /npm-staged-package-proofs/)
  assert.match(src, /downloadStageProof\(runId\)/)
  assert.match(src, /requirePinnedCandidate\(state\)/)
  assert.match(src, /ensureTagAndRelease\(gate\.version, candidate\.sha\)/)
  assert.match(workflow, /\[ "\$REF_NAME" = "main" \]/)
  assert.match(workflow, /"\$REF" != refs\/heads\/\*/)
  assert.match(workflow, /gh api --paginate --slurp/)
})

test('release runbook pins the staged-publish OIDC identity and action', () => {
  const runbook = readFileSync(
    path.join(PUBLISH_DIR, '../../docs/src/contributing/releasing.md'),
    'utf8',
  )
  assert.match(runbook, /workflow filename: `npm-stage-publish\.yml`/)
  assert.match(runbook, /environment: `npm-publish`/)
  assert.match(runbook, /allowed action: \*\*`npm stage publish`\*\*/)
  assert.match(runbook, /npm permits only one trusted publisher per package/)
})

test('planReleaseNotes: oversized notes move to a stable release asset', () => {
  const inline = planReleaseNotes('0.5.1519', 'small notes')
  assert.deepEqual(inline, { body: 'small notes', attachFullNotes: false })

  const large = planReleaseNotes(
    '0.5.1519',
    'x'.repeat(INLINE_RELEASE_NOTES_MAX_BYTES + 1),
  )
  assert.equal(large.attachFullNotes, true)
  assert.match(large.body, /release-notes-full\.md/)
  assert.match(large.body, /releases\/download\/v0\.5\.1519\/release-notes-full\.md/)
  assert.ok(Buffer.byteLength(large.body, 'utf8') < INLINE_RELEASE_NOTES_MAX_BYTES)
})

test('pipeline.mts: two conflicting mode flags fail closed instead of picking one by argument order', () => {
  // Mode resolution used to be `flags.find(...)`, which silently picked
  // whichever mode flag argv happened to list first — so `--stage-only
  // --approve` ran --stage-only (or, with the args reversed, --approve)
  // instead of refusing an ambiguous invocation. This spawns the real CLI:
  // the conflict must be caught before any gh/network call, so this is safe
  // to exercise directly rather than only asserting against the source text.
  const result = spawnSync(
    process.execPath,
    [path.join(PUBLISH_DIR, 'pipeline.mts'), '--stage-only', '--approve'],
    { encoding: 'utf8' },
  )
  assert.equal(result.status, 1, 'conflicting mode flags must exit non-zero')
  const output = `${result.stdout}${result.stderr}`
  assert.match(
    output,
    /Conflicting mode flags/,
    'pipeline.mts must name the conflict rather than silently choosing a mode',
  )
})

test('pipeline.mts / approve.mts: the socket scan gate has no skip flag', () => {
  for (const rel of ['pipeline.mts', 'npm/approve.mts']) {
    const src = readFileSync(path.join(PUBLISH_DIR, rel), 'utf8')
    assert.doesNotMatch(
      src,
      /no-scan|noScan/,
      `${rel}: the Socket scan gate must not have a skip flag/option — it is mandatory`,
    )
  }
})

test('NPM_MIN_VERSION floor covers staged publishing + min-release-age', () => {
  // `npm stage` landed in 11.15.0; min-release-age (DAYS) needs >= 11.17.
  // The floor must be at least both (and OIDC's 11.5.1).
  assert.ok(
    compareSemver(NPM_MIN_VERSION, '11.15.0') >= 0,
    `floor ${NPM_MIN_VERSION} must cover npm stage (>= 11.15.0)`,
  )
  assert.ok(
    compareSemver(NPM_MIN_VERSION, '11.17.0') >= 0,
    `floor ${NPM_MIN_VERSION} must cover min-release-age in DAYS (>= 11.17)`,
  )
  assert.ok(
    compareSemver(NPM_MIN_VERSION, '11.5.1') >= 0,
    `floor ${NPM_MIN_VERSION} must cover OIDC trusted publishing (>= 11.5.1)`,
  )
})
