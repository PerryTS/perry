#!/usr/bin/env node
/**
 * @file Node.js builtin-module COMPATIBILITY MATRIX harness.
 *
 *   Perry reimplements the `node:*` module surface natively. This harness
 *   measures — against a PINNED, verified Node (the oracle) — how faithfully
 *   Perry reproduces each builtin's EXPORT SHAPE, for BOTH import forms
 *   (`M` and `node:M`). Its value is BREADTH: every builtin, both forms,
 *   one pinned oracle. Behavioral parity lives in the hand-authored
 *   node-suite (run_parity_tests.sh); this is a wide, shallow shape sweep.
 *
 *   Design choices (see docs/src/testing/node-compat-matrix.md):
 *     - BINARY pin, not a source checkout: the oracle is the official
 *       nodejs.org dist tarball for the host platform, pinned in
 *       external-tools.json with a sha512 SRI and download-verified here.
 *       We measure shape against the SHIPPED runtime, not a self-build.
 *     - Node ESM runner, not bash: the matrix is data-structured (per
 *       module x per form, a JSON baseline, SRI download/verify, a printed
 *       table). run_parity_tests.sh streams a single node-vs-perry compare
 *       beautifully; this needs records, not streams. We DO reuse its
 *       compile+run contract (PERRY_ALLOW_UNIMPLEMENTED=1, the
 *       --enable-js-runtime retry, PERRY_STUB_DIAG=off) — and sidestep its
 *       output-normalization entirely with a sentinel-wrapped fingerprint
 *       line, so environmental warnings never touch the compare.
 *
 *   FINGERPRINT (per module, per form): the probe does `import * as ns`,
 *   sorts `Object.keys(ns)`, and emits `name:typeof` for each key plus the
 *   default export's typeof, wrapped in `__FP__...__FP__`. Equal
 *   fingerprints == equal export shape. It is a SHAPE fingerprint (names +
 *   typeofs), not deep behavior.
 *
 *   MODES:
 *     (default)            run the full matrix, print a table + summary
 *     --check              compare to the committed baseline; exit 1 on
 *                          regressions (a cell that got strictly worse, or a
 *                          prefix-parity invariant that broke)
 *     --update-baseline    rewrite test-parity/node-compat-matrix.baseline.json
 *     --node-version <ver> use a different Node line (e.g. the LTS row);
 *                          verified via that version's SHASUMS256.txt
 *     --json               also print the raw result object as JSON
 *
 *   FAST-LOOP SELECTORS (narrow to a sub-second inner loop — the pinned
 *   node download is skipped once cached):
 *     --module <a,b,c>     restrict to a comma-separated set of base modules
 *     --method <a,b>       with --module, fingerprint ONLY those exports of
 *                          the selected module(s) — e.g.
 *                          `--module fs --method readFileSync,promises`
 *     --only <mod.export>  combined form (comma-separated), e.g.
 *                          `--only fs.readFileSync,path.join`
 *     A module selector makes --check / --update-baseline operate on JUST
 *     that slice (never silently rewriting the whole baseline). A method
 *     subset changes the fingerprint semantics, so it is a print-only fast
 *     diagnostic and is refused for --check / --update-baseline.
 *
 *   The prefixed/unprefixed INVARIANT: Node treats `M` and `node:M`
 *   identically for builtins, and Perry's is_native_module strips the
 *   `node:` prefix, so the two forms MUST agree. Any divergence is a real
 *   Perry bug and is flagged (prefixParity=false).
 */

import { createHash } from 'node:crypto'
import { spawnSync } from 'node:child_process'
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import process from 'node:process'
import { fileURLToPath } from 'node:url'

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const EXTERNAL_TOOLS_JSON = path.join(REPO_ROOT, 'external-tools.json')
const BASELINE_PATH = path.join(REPO_ROOT, 'test-parity', 'node-compat-matrix.baseline.json')
const SKIP_PATH = path.join(REPO_ROOT, 'test-parity', 'node-compat-matrix.skip.json')
const CACHE_ROOT = path.join(REPO_ROOT, '.cache', 'node-pin')
const MANIFEST_ENTRIES = path.join(
  REPO_ROOT,
  'crates',
  'perry-api-manifest',
  'src',
  'entries.rs',
)
const PERRY_BIN = path.join(REPO_ROOT, 'target', 'release', 'perry')

// Compile can be slow on the FIRST call (it builds the auto-optimized
// runtime once), then warm calls are sub-second. Runs are tiny.
const COMPILE_TIMEOUT_MS = 300_000
const RUN_TIMEOUT_MS = 15_000
const NODE_TIMEOUT_MS = 15_000

// --- CLI -------------------------------------------------------------------

function parseArgs(argv) {
  // moduleSet: null = all builtins; otherwise the selected base modules.
  // methodMap: base -> [export names] to fingerprint (subset mode).
  const args = {
    mode: 'run',
    moduleSet: null,
    methodMap: new Map(),
    globalMethods: [],
    nodeVersion: null,
    json: false,
  }
  const modules = new Set()
  const splitList = s => (s || '').split(',').map(x => x.trim()).filter(Boolean)
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i]
    if (a === '--check') args.mode = 'check'
    else if (a === '--update-baseline') args.mode = 'update'
    else if (a === '--json') args.json = true
    else if (a === '--module') {
      for (const m of splitList(argv[++i])) modules.add(m.replace(/^node:/, ''))
    } else if (a === '--method') {
      args.globalMethods.push(...splitList(argv[++i]))
    } else if (a === '--only') {
      // mod.export pairs: fs.readFileSync,path.join
      for (const pair of splitList(argv[++i])) {
        const dot = pair.indexOf('.')
        if (dot < 0) {
          console.error(`[node-compat] --only expects mod.export, got: ${pair}`)
          args.mode = 'help'
          continue
        }
        const base = pair.slice(0, dot).replace(/^node:/, '')
        const method = pair.slice(dot + 1)
        modules.add(base)
        if (!args.methodMap.has(base)) args.methodMap.set(base, [])
        args.methodMap.get(base).push(method)
      }
    } else if (a === '--node-version') args.nodeVersion = argv[++i]
    else if (a === '--help' || a === '-h') args.mode = 'help'
    else {
      console.error(`[node-compat] unknown argument: ${a}`)
      args.mode = 'help'
    }
  }
  if (modules.size > 0) args.moduleSet = modules
  // Fold a global --method list onto each explicitly selected module.
  if (args.globalMethods.length > 0 && args.moduleSet) {
    for (const base of args.moduleSet) {
      const cur = args.methodMap.get(base) || []
      args.methodMap.set(base, [...cur, ...args.globalMethods])
    }
  }
  args.subsetActive = args.methodMap.size > 0
  return args
}

const HELP = `node_compat_matrix.mjs — Node builtin-module compatibility matrix

  # FAST LOOP (reach for this while iterating on one builtin):
  node scripts/node_compat_matrix.mjs --module fs
  node scripts/node_compat_matrix.mjs --module fs,path,crypto
  node scripts/node_compat_matrix.mjs --module fs --method readFileSync,promises
  node scripts/node_compat_matrix.mjs --only fs.readFileSync,path.join

  # FULL SWEEP + GATE:
  node scripts/node_compat_matrix.mjs                 run + print table
  node scripts/node_compat_matrix.mjs --check         gate against baseline
  node scripts/node_compat_matrix.mjs --update-baseline
  node scripts/node_compat_matrix.mjs --node-version 24.18.1
  node scripts/node_compat_matrix.mjs --json

A --module selector makes --check / --update-baseline touch only that slice.
A --method / --only subset is a print-only fast diagnostic (it changes the
fingerprint semantics, so it is refused for --check / --update-baseline).

The pinned Node version lives in external-tools.json (tools.node.version).
Bump it there, run --update-baseline, and review the diff.`

// --- pin + platform --------------------------------------------------------

function loadNodePin() {
  const pin = JSON.parse(readFileSync(EXTERNAL_TOOLS_JSON, 'utf8')).tools.node
  if (!pin) throw new Error('external-tools.json has no "node" pin')
  return pin
}

function platformKey() {
  const okey = { darwin: 'darwin', linux: 'linux', win32: 'win' }[process.platform]
  const akey = { arm64: 'arm64', x64: 'x64' }[process.arch]
  if (!okey || !akey) throw new Error(`unsupported platform ${process.platform}-${process.arch}`)
  return `${okey}-${akey}`
}

function sriSha512(buf) {
  return `sha512-${createHash('sha512').update(buf).digest('base64')}`
}

function sha256hex(buf) {
  return createHash('sha256').update(buf).digest('hex')
}

async function fetchBuffer(url) {
  const res = await fetch(url, { redirect: 'follow', signal: AbortSignal.timeout(180_000) })
  if (!res.ok) throw new Error(`download failed ${res.status} ${url}`)
  return Buffer.from(await res.arrayBuffer())
}

/**
 * Resolve the pinned (or --node-version) Node to an executable path,
 * downloading + verifying + caching under .cache/node-pin/ as needed.
 * Pinned line: verify against the sha512 SRI in external-tools.json.
 * Non-pinned line: verify against that version's SHASUMS256.txt (sha256).
 */
async function resolveNode(pin, versionOverride) {
  const version = versionOverride || pin.version
  const key = platformKey()
  const [okey, akey] = key.split('-')
  const ext = okey === 'win' ? 'zip' : 'tar.gz'
  const asset = `node-v${version}-${okey}-${akey}.${ext}`
  const dirName = `node-v${version}-${okey}-${akey}`
  const versionDir = path.join(CACHE_ROOT, version)
  const extractedDir = path.join(versionDir, dirName)
  const nodeBin =
    okey === 'win'
      ? path.join(extractedDir, 'node.exe')
      : path.join(extractedDir, 'bin', 'node')

  if (existsSync(nodeBin)) return nodeBin

  const base = pin.distBaseUrl || 'https://nodejs.org/dist'
  const url = `${base}/v${version}/${asset}`
  console.error(`[node-compat] resolving Node ${version} (${key}) ...`)
  const buf = await fetchBuffer(url)

  const isPinned = version === pin.version
  const platPin = pin.platforms?.[key]
  // A version matching the pin MUST have a per-platform sha512 SRI. Falling
  // through to the SHASUMS256.txt path here would silently downgrade the
  // strongest guarantee (repo-committed SRI) to a weaker one whenever a
  // platform pin is missing — e.g. a new host arch, or a bump that forgot a
  // platform. Fail loudly instead of skipping the pin without any signal.
  if (isPinned && !platPin) {
    throw new Error(
      `no sha512 SRI pin for ${key} in external-tools.json tools.node.platforms — add one before probing the pinned line`,
    )
  }
  if (isPinned) {
    const actual = sriSha512(buf)
    if (actual !== platPin.integrity) {
      throw new Error(
        `integrity mismatch for ${url}\n  expected ${platPin.integrity}\n  actual   ${actual}`,
      )
    }
    console.error(`[node-compat] verified ${asset} against pinned sha512 SRI`)
  } else {
    // Non-pinned line (e.g. LTS row): verify sha256 against the dist's
    // published SHASUMS256.txt. No sha512 pin exists for it in this repo.
    const sums = (await fetchBuffer(`${base}/v${version}/SHASUMS256.txt`)).toString('utf8')
    const want = sums
      .split('\n')
      .map(l => l.trim().split(/\s+/))
      .find(([, name]) => name === asset)?.[0]
    if (!want) throw new Error(`${asset} not found in SHASUMS256.txt for v${version}`)
    const got = sha256hex(buf)
    if (got !== want) {
      throw new Error(`sha256 mismatch for ${url}\n  expected ${want}\n  actual   ${got}`)
    }
    console.error(`[node-compat] verified ${asset} against SHASUMS256.txt (sha256, unpinned line)`)
  }

  mkdirSync(versionDir, { recursive: true })
  const archivePath = path.join(versionDir, asset)
  writeFileSync(archivePath, buf)
  const tarBin =
    process.platform === 'win32'
      ? path.join(process.env.SystemRoot || 'C:\\Windows', 'System32', 'tar.exe')
      : 'tar'
  const flags = ext === 'zip' ? '-xf' : '-xzf'
  const ex = spawnSync(tarBin, [flags, asset], { cwd: versionDir, stdio: 'inherit' })
  rmSync(archivePath, { force: true })
  if (ex.status !== 0) throw new Error('node dist extract failed')
  if (!existsSync(nodeBin)) throw new Error(`node binary not found at ${nodeBin} after extract`)
  console.error(`[node-compat] cached Node ${version} -> ${nodeBin}`)
  return nodeBin
}

// --- probe + fingerprint ---------------------------------------------------

const FP_RE = /__FP__([\s\S]*?)__FP__/

function probeSource(spec, methods) {
  // A single sentinel-wrapped line carries the shape fingerprint, so any
  // node/perry warnings on stdout/stderr are simply ignored by the extractor.
  // With a method subset, fingerprint ONLY those exports (a fast diagnostic
  // loop); otherwise fingerprint the whole sorted namespace.
  const keysExpr =
    methods && methods.length > 0
      ? `const keys = ${JSON.stringify([...methods].sort())}`
      : `const keys = Object.keys(rec).sort()`
  return [
    `import * as ns from ${JSON.stringify(spec)}`,
    `const rec = ns as unknown as Record<string, unknown>`,
    keysExpr,
    `const parts: string[] = []`,
    `for (const k of keys) parts.push(k + ":" + typeof rec[k])`,
    `const dflt = (rec as unknown as { default?: unknown }).default`,
    `console.log("__FP__" + parts.join(",") + "|default=" + typeof dflt + "__FP__")`,
    ``,
  ].join('\n')
}

function extractFp(output) {
  const m = FP_RE.exec(output)
  return m ? m[1] : null
}

function fpHash(fp) {
  return fp === null ? '' : createHash('sha256').update(fp).digest('hex').slice(0, 12)
}

/** Ask the pinned Node (oracle) for a module form's fingerprint. */
function oracleFingerprint(nodeBin, probeFile) {
  const res = spawnSync(
    nodeBin,
    ['--experimental-strip-types', probeFile],
    {
      encoding: 'utf8',
      timeout: NODE_TIMEOUT_MS,
      env: { ...process.env, FORCE_COLOR: '0', NO_COLOR: '1', NODE_DISABLE_COLORS: '1' },
    },
  )
  return extractFp(`${res.stdout || ''}\n${res.stderr || ''}`)
}

/** Compile with Perry, run, return the fingerprint (or null on any failure). */
function perryFingerprint(probeFile, outBin) {
  const compileEnv = { ...process.env, PERRY_ALLOW_UNIMPLEMENTED: '1' }
  let c = spawnSync(PERRY_BIN, [probeFile, '-o', outBin], {
    encoding: 'utf8',
    timeout: COMPILE_TIMEOUT_MS,
    env: compileEnv,
  })
  const cout = `${c.stdout || ''}\n${c.stderr || ''}`
  // Mirror run_parity_tests.sh: retry once with the JS-runtime host opt-in
  // when the failure names perry-jsruntime.
  if (c.status !== 0 && /perry-jsruntime/.test(cout)) {
    c = spawnSync(PERRY_BIN, ['--enable-js-runtime', probeFile, '-o', outBin], {
      encoding: 'utf8',
      timeout: COMPILE_TIMEOUT_MS,
      env: compileEnv,
    })
  }
  if (c.status !== 0 || !existsSync(outBin)) return null
  const r = spawnSync(outBin, [], {
    encoding: 'utf8',
    timeout: RUN_TIMEOUT_MS,
    env: { ...process.env, PERRY_STUB_DIAG: 'off' },
  })
  rmSync(outBin, { force: true })
  if (r.status !== 0) return null
  return extractFp(`${r.stdout || ''}\n${r.stderr || ''}`)
}

// --- status model ----------------------------------------------------------

// Lower is better. --check flags a cell whose severity increased.
const SEVERITY = {
  skip: -1,
  match: 0,
  'perry-extra': 0, // Perry resolves a form Node's oracle didn't — not a regression.
  'both-unresolved': 1, // Node can't import it either; neutral.
  'shape-diff': 2,
  'perry-unresolved': 3,
}

function cellStatus(oracleFp, perryFp, skipped) {
  if (skipped) return 'skip'
  const oracleOk = oracleFp !== null
  const perryOk = perryFp !== null
  if (!oracleOk && !perryOk) return 'both-unresolved'
  if (oracleOk && !perryOk) return 'perry-unresolved'
  if (!oracleOk && perryOk) return 'perry-extra'
  return oracleFp === perryFp ? 'match' : 'shape-diff'
}

// --- manifest cross-check --------------------------------------------------

function extractRustArray(src, name) {
  const start = src.indexOf(`${name}: &[&str] = &[`)
  if (start < 0) return []
  const end = src.indexOf('];', start)
  const body = src.slice(start, end)
  const out = new Set()
  for (const m of body.matchAll(/"([^"]+)"/g)) out.add(m[1])
  return [...out]
}

function loadManifestModules() {
  const src = readFileSync(MANIFEST_ENTRIES, 'utf8')
  const native = extractRustArray(src, 'NATIVE_MODULES')
  const submodules = extractRustArray(src, 'NODE_SUBMODULES')
  return new Set([...native, ...submodules])
}

// --- enumerate builtins ----------------------------------------------------

function enumerateBuiltins(nodeBin) {
  const res = spawnSync(
    nodeBin,
    ['-e', "process.stdout.write(require('module').builtinModules.join('\\n'))"],
    { encoding: 'utf8', timeout: NODE_TIMEOUT_MS },
  )
  if (res.status !== 0) throw new Error(`could not enumerate builtinModules: ${res.stderr}`)
  const bases = new Set()
  for (const raw of res.stdout.split('\n')) {
    const name = raw.trim()
    if (!name) continue
    const base = name.replace(/^node:/, '')
    // Skip internals never meant as public import specifiers.
    if (base.startsWith('internal/') || base.startsWith('_')) continue
    bases.add(base)
  }
  return [...bases].sort()
}

function loadSkip() {
  if (!existsSync(SKIP_PATH)) return {}
  return JSON.parse(readFileSync(SKIP_PATH, 'utf8')).modules || {}
}

// --- run the matrix --------------------------------------------------------

async function runMatrix(args) {
  if (!existsSync(PERRY_BIN)) {
    throw new Error(
      `perry release binary missing at ${PERRY_BIN}\n  build it: cargo build --release -p perry`,
    )
  }
  const pin = loadNodePin()
  const nodeBin = await resolveNode(pin, args.nodeVersion)
  const version = args.nodeVersion || pin.version
  const skip = loadSkip()

  let bases = enumerateBuiltins(nodeBin)
  if (args.moduleSet) {
    const requested = [...args.moduleSet]
    const known = new Set(bases)
    const missing = requested.filter(b => !known.has(b))
    if (missing.length > 0) {
      throw new Error(`not builtins of Node ${version}: ${missing.join(', ')}`)
    }
    bases = bases.filter(b => args.moduleSet.has(b))
  }

  const tmp = mkdtempSync(path.join(os.tmpdir(), 'perry-node-compat-'))
  const results = {}
  let done = 0
  for (const base of bases) {
    done++
    process.stderr.write(`\r[node-compat] ${done}/${bases.length} ${base.padEnd(24)}`)
    const entry = {}
    const methods = args.methodMap.get(base)
    for (const [form, spec] of [
      ['unprefixed', base],
      ['prefixed', `node:${base}`],
    ]) {
      const skipRec = skip[base]
      const skipped = Boolean(skipRec)
      const probeFile = path.join(tmp, `probe_${base.replace(/[^\w]/g, '_')}_${form}.ts`)
      writeFileSync(probeFile, probeSource(spec, methods))
      let oracleFp = null
      let perryFp = null
      if (!skipped) {
        oracleFp = oracleFingerprint(nodeBin, probeFile)
        const outBin = path.join(tmp, `bin_${base.replace(/[^\w]/g, '_')}_${form}`)
        perryFp = perryFingerprint(probeFile, outBin)
      }
      entry[form] = {
        status: cellStatus(oracleFp, perryFp, skipped),
        node: oracleFp !== null ? 'ok' : skipped ? 'skip' : 'throw',
        perry: perryFp !== null ? 'ok' : skipped ? 'skip' : 'unresolved',
        nodeFp: fpHash(oracleFp),
        perryFp: fpHash(perryFp),
      }
    }
    // Prefix-parity invariant: when Node supports BOTH M and node:M, Perry
    // must produce the same result for both forms. Prefix-only Node builtins
    // still probe the bare spelling (where Perry may be more permissive), but
    // that perry-extra cell is not part of the parity invariant.
    const u = entry.unprefixed
    const p = entry.prefixed
    entry.prefixParity =
      u.node === 'ok' && p.node === 'ok'
        ? u.perry === p.perry && u.perryFp === p.perryFp
        : null
    if (skip[base]) entry.skipReason = skip[base].reason
    results[base] = entry
  }
  process.stderr.write('\r' + ' '.repeat(60) + '\r')
  rmSync(tmp, { recursive: true, force: true })

  return { version, platform: platformKey(), modules: results }
}

// --- reporting -------------------------------------------------------------

const GLYPH = {
  match: 'match',
  'shape-diff': 'SHAPE-DIFF',
  'perry-unresolved': 'UNRESOLVED',
  'perry-extra': 'perry-extra',
  'both-unresolved': 'both-none',
  skip: 'skip',
}

function summarize(matrix, manifestModules) {
  const mods = Object.entries(matrix.modules)
  const total = mods.length
  let bothMatch = 0
  const perryUnresolved = []
  const shapeDiff = []
  const prefixDivergences = []
  const claimedButBroken = []
  const worksButUnclaimed = []
  const skipped = []

  for (const [base, e] of mods) {
    if (e.unprefixed.status === 'skip') {
      skipped.push(base)
      continue
    }
    const u = e.unprefixed.status
    const p = e.prefixed.status
    if (u === 'match' && p === 'match') bothMatch++
    if (u === 'perry-unresolved' || p === 'perry-unresolved') perryUnresolved.push(base)
    if (u === 'shape-diff' || p === 'shape-diff') shapeDiff.push(base)
    if (!e.prefixParity) prefixDivergences.push(base)

    const claimed = manifestModules.has(base)
    const perryResolvesEither = e.unprefixed.perry === 'ok' || e.prefixed.perry === 'ok'
    if (claimed && !perryResolvesEither) claimedButBroken.push(base)
    if (!claimed && perryResolvesEither) worksButUnclaimed.push(base)
  }
  return {
    total,
    bothMatch,
    perryUnresolved,
    shapeDiff,
    prefixDivergences,
    claimedButBroken,
    worksButUnclaimed,
    skipped,
  }
}

function printTable(matrix) {
  const rows = Object.entries(matrix.modules)
  const w = Math.max(...rows.map(([b]) => b.length), 6)
  console.log('')
  console.log(`${'MODULE'.padEnd(w)}  ${'unprefixed'.padEnd(11)}  ${'node:'.padEnd(11)}  parity`)
  console.log('-'.repeat(w + 2 + 11 + 2 + 11 + 2 + 6))
  for (const [base, e] of rows) {
    const parity = e.unprefixed.status === 'skip' ? '-' : e.prefixParity ? 'ok' : 'DIVERGE'
    console.log(
      `${base.padEnd(w)}  ${GLYPH[e.unprefixed.status].padEnd(11)}  ${GLYPH[e.prefixed.status].padEnd(11)}  ${parity}`,
    )
  }
}

function printSummary(s, matrix) {
  console.log('')
  console.log(`Node oracle:        v${matrix.version} (${matrix.platform})`)
  console.log(`Builtins probed:    ${s.total}`)
  console.log(`Both-forms match:   ${s.bothMatch}`)
  console.log(`Shape-diff:         ${s.shapeDiff.length}${s.shapeDiff.length ? ' (' + s.shapeDiff.join(', ') + ')' : ''}`)
  console.log(`Perry-unresolved:   ${s.perryUnresolved.length}${s.perryUnresolved.length ? ' (' + s.perryUnresolved.join(', ') + ')' : ''}`)
  console.log(`Skipped (curated):  ${s.skipped.length}${s.skipped.length ? ' (' + s.skipped.join(', ') + ')' : ''}`)
  console.log(`Prefix divergences: ${s.prefixDivergences.length}${s.prefixDivergences.length ? ' (' + s.prefixDivergences.join(', ') + ')' : ''}`)
  console.log(`Claimed-but-broken: ${s.claimedButBroken.length}${s.claimedButBroken.length ? ' (' + s.claimedButBroken.join(', ') + ')' : ''}  (in NATIVE_MODULES/NODE_SUBMODULES yet perry-unresolved both forms)`)
  console.log(`Works-but-unclaimed:${s.worksButUnclaimed.length}${s.worksButUnclaimed.length ? ' (' + s.worksButUnclaimed.join(', ') + ')' : ''}  (perry resolves it but it is not in the manifest lists)`)
}

// --- baseline read/write/check ---------------------------------------------

const BASELINE_SCHEMA = {
  description:
    'Per-module x per-form (unprefixed / node:-prefixed) export-shape status for the Node builtin-module compatibility matrix. Generated by scripts/node_compat_matrix.mjs against the pinned Node oracle (external-tools.json tools.node). --check fails on any cell that got strictly worse or any prefix-parity invariant that broke; improvements are accepted. Regenerate with --update-baseline and review the diff.',
  statuses: {
    match: 'perry export fingerprint == node oracle fingerprint',
    'shape-diff': 'both resolved, fingerprints differ (a real shape gap)',
    'perry-unresolved': 'node resolved, perry did not compile/run',
    'perry-extra': 'perry resolved a form the node oracle did not',
    'both-unresolved': 'neither node nor perry resolved the form (neutral)',
    skip: 'curated skip (see test-parity/node-compat-matrix.skip.json)',
  },
  fields: {
    prefixParity:
      'true/false when Node resolves both M and node:M and Perry produces the same/different result; null for Node prefix-only builtins.',
    nodeFp: 'first 12 hex of sha256(oracle fingerprint)',
    perryFp: 'first 12 hex of sha256(perry fingerprint)',
  },
}

function toBaseline(matrix) {
  return {
    _schema: BASELINE_SCHEMA,
    nodeVersion: matrix.version,
    platform: matrix.platform,
    modules: matrix.modules,
  }
}

function writeBaseline(matrix, partial) {
  let doc = toBaseline(matrix)
  if (partial && existsSync(BASELINE_PATH)) {
    // Selector-scoped update: overlay ONLY the probed modules onto the
    // committed baseline so a single-module refresh never rewrites the
    // whole file. The version/platform header follows the probed run.
    const prev = JSON.parse(readFileSync(BASELINE_PATH, 'utf8'))
    const merged = { ...prev.modules, ...matrix.modules }
    // Deterministic key order.
    const modules = {}
    for (const k of Object.keys(merged).sort()) modules[k] = merged[k]
    doc = { ...doc, modules }
  }
  writeFileSync(BASELINE_PATH, `${JSON.stringify(doc, null, 2)}\n`)
  const scope = partial ? ` (merged ${Object.keys(matrix.modules).length} module slice)` : ''
  console.error(`[node-compat] wrote baseline ${path.relative(REPO_ROOT, BASELINE_PATH)}${scope}`)
}

function baselineValidationError(base) {
  if (!base || typeof base !== 'object' || Array.isArray(base)) {
    return 'baseline root must be an object'
  }
  if (!base.modules || typeof base.modules !== 'object' || Array.isArray(base.modules)) {
    return 'baseline modules must be an object'
  }
  for (const [name, entry] of Object.entries(base.modules)) {
    if (!entry || typeof entry !== 'object' || Array.isArray(entry)) {
      return `baseline module ${name} must be an object`
    }
    for (const form of ['unprefixed', 'prefixed']) {
      const status = entry[form]?.status
      if (typeof status !== 'string' || !Object.hasOwn(SEVERITY, status)) {
        return `baseline module ${name} is missing a valid ${form}.status`
      }
    }
    if (entry.prefixParity !== null && typeof entry.prefixParity !== 'boolean') {
      return `baseline module ${name} prefixParity must be boolean or null`
    }
  }
  return null
}

function checkAgainstBaseline(matrix) {
  if (!existsSync(BASELINE_PATH)) {
    console.error(`[node-compat] no baseline at ${BASELINE_PATH} — run --update-baseline first`)
    return 1
  }
  const base = JSON.parse(readFileSync(BASELINE_PATH, 'utf8'))
  const validationError = baselineValidationError(base)
  if (validationError) {
    console.error(`[node-compat] invalid baseline: ${validationError}`)
    return 1
  }
  // A baseline is scoped to the platform AND Node line it was generated
  // against — the fingerprints of platform-dependent surfaces (os, path/win32,
  // dgram, fs, inspector) and version-dependent export shapes only compare
  // meaningfully within that scope. Comparing a darwin-arm64 baseline against a
  // linux-x64 run (the nightly job's old ubuntu runner), or a --node-version
  // override against the pin-generated baseline, produces phantom regressions —
  // or masks real ones. Refuse across-scope comparison instead of comparing
  // cells blindly.
  if (base.platform !== matrix.platform) {
    console.error(
      `[node-compat] baseline platform ${base.platform} != this run ${matrix.platform} — run the gate on a ${base.platform} runner, or regenerate the baseline on ${matrix.platform} (--update-baseline)`,
    )
    return 1
  }
  if (base.nodeVersion !== matrix.version) {
    console.error(
      `[node-compat] baseline nodeVersion ${base.nodeVersion} != this run ${matrix.version} — --check compares only against the pinned oracle; bump the pin and --update-baseline, or drop the --node-version override`,
    )
    return 1
  }
  // A green-but-empty gate is worse than a red one: CLAUDE.md "four ways a gate
  // can be unable to fail" #4 — the gate ran but its subject didn't. Prove the
  // sweep actually processed its breadth before trusting a pass, so a future
  // selector/enumeration regression can't leave a vacuously green gate.
  const processed = Object.keys(matrix.modules).length
  const expected = Object.keys(base.modules).length
  if (processed === 0) {
    console.error('[node-compat] sweep processed 0 modules — refusing a vacuous pass')
    return 1
  }
  // Symmetric guard on the OTHER operand: an empty baseline makes `expected` 0,
  // so `processed < expected` can never trip and every current module reads as
  // "new" — the gate goes green comparing against nothing (the "swept N/0"
  // tell). A truncated or merge-emptied committed baseline would silently
  // disable the gate forever. Same doctrine (#4: subject didn't run), other side.
  if (expected === 0) {
    console.error(
      '[node-compat] baseline has 0 modules — empty/corrupt baseline, refusing a vacuous pass (regenerate with --update-baseline)',
    )
    return 1
  }
  if (!matrix.__partial && processed < expected) {
    console.error(
      `[node-compat] full sweep processed only ${processed} of ${expected} baselined modules — partial sweep, refusing a vacuous pass (use a --module selector for an intentionally scoped check)`,
    )
    return 1
  }
  const regressions = []
  for (const [name, cur] of Object.entries(matrix.modules)) {
    const prev = base.modules[name]
    if (!prev) continue // new module (e.g. after a node bump) — not a regression
    for (const form of ['unprefixed', 'prefixed']) {
      const wasSev = SEVERITY[prev[form].status]
      const nowSev = SEVERITY[cur[form]?.status] ?? 0
      if (nowSev > wasSev) {
        regressions.push(`${name} [${form}]: ${prev[form]?.status} -> ${cur[form]?.status}`)
      }
    }
    if (prev.prefixParity === true && cur.prefixParity === false) {
      regressions.push(`${name}: prefix-parity broke (M and node:M now diverge)`)
    }
  }
  // Modules dropped from the matrix that were passing before also count.
  for (const [name, prev] of Object.entries(base.modules)) {
    if (!matrix.modules[name] && !matrix.__partial) {
      if ((SEVERITY[prev.unprefixed?.status] ?? 0) === 0 && (SEVERITY[prev.prefixed?.status] ?? 0) === 0) {
        regressions.push(`${name}: was in baseline (matching) but no longer probed`)
      }
    }
  }
  if (regressions.length === 0) {
    console.log(
      `[node-compat] OK — no regressions vs baseline (Node v${base.nodeVersion}, ${base.platform}, swept ${processed}/${expected} modules)`,
    )
    return 0
  }
  console.error(`[node-compat] REGRESSIONS (${regressions.length}):`)
  for (const r of regressions) console.error(`  - ${r}`)
  return 1
}

// --- main ------------------------------------------------------------------

async function main() {
  const args = parseArgs(process.argv.slice(2))
  if (args.mode === 'help') {
    console.log(HELP)
    return 0
  }
  const partial = Boolean(args.moduleSet)
  // A method subset changes fingerprint semantics — it can never touch the
  // committed baseline (its fingerprints are not comparable to full-shape
  // ones). Refuse before doing the work.
  if (args.subsetActive && (args.mode === 'check' || args.mode === 'update')) {
    console.error(
      '[node-compat] --method/--only is a print-only fast diagnostic; use a module-level selector (no --method/--only) for --check / --update-baseline',
    )
    return 1
  }

  const matrix = await runMatrix(args)
  if (partial) matrix.__partial = true
  const manifestModules = loadManifestModules()
  const summary = summarize(matrix, manifestModules)

  if (args.mode === 'update') {
    writeBaseline(matrix, partial)
    printTable(matrix)
    if (!partial) printSummary(summary, matrix)
    return 0
  }

  if (args.mode === 'check') {
    if (args.json) console.log(JSON.stringify(toBaseline(matrix), null, 2))
    return checkAgainstBaseline(matrix)
  }

  // default: run + report
  printTable(matrix)
  printSummary(summary, matrix)
  if (args.json) console.log(JSON.stringify(toBaseline(matrix), null, 2))
  return 0
}

main().then(
  code => {
    process.exitCode = code
  },
  err => {
    console.error(`[node-compat] ${err.stack || err.message}`)
    process.exitCode = 1
  },
)
