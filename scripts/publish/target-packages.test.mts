/**
 * @file Tests for the per-target npm packages: the publish order, the staged
 *   package shape (stage-target.mts), and stage-npm.sh's refusal to stage a
 *   release with a missing or tampered cross bundle. Run with
 *   `node --test scripts/publish/target-packages.test.mts`.
 */

import { test } from 'node:test'
import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import {
  cpSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'

import {
  ALL_PACKAGES,
  PLATFORM_PACKAGES,
  REPO_ROOT,
  TARGET_PACKAGES,
  TARGET_TRIPLES,
  WRAPPER_PACKAGE,
  npmPackageDir,
  targetPackageName,
} from './constants.mts'
import { stageTarget } from './stage-target.mts'

const VERSION = '0.5.1700'
const REVISION = 'a'.repeat(40)

function sha256(bytes: Buffer | string): string {
  return createHash('sha256').update(bytes).digest('hex')
}

/** An extracted cross bundle: archives + manifest.json, as build-cross writes it. */
function bundle(
  dir: string,
  triple: string,
  { revision = REVISION as string | undefined, files = ['libperry_runtime.a', 'libperry_runtime.a.gc-effects'] } = {},
): string {
  mkdirSync(dir, { recursive: true })
  const entries = files.map(name => {
    const bytes = Buffer.from(`${triple}:${name}`)
    writeFileSync(path.join(dir, name), bytes)
    return { path: name, sha256: sha256(bytes), size: bytes.length }
  })
  const manifest: Record<string, unknown> = {
    schema_version: 1,
    perry_version: VERSION,
    target_triple: triple,
    files: entries,
  }
  if (revision) manifest.source_revision = revision
  writeFileSync(path.join(dir, 'manifest.json'), JSON.stringify(manifest))
  return dir
}

function withTemp(fn: (dir: string) => void): void {
  const dir = mkdtempSync(path.join(os.tmpdir(), 'perry-target-test-'))
  try {
    fn(dir)
  } finally {
    rmSync(dir, { recursive: true, force: true })
  }
}

test('ALL_PACKAGES: hosts, then targets, then the wrapper', () => {
  assert.deepEqual(ALL_PACKAGES, [...PLATFORM_PACKAGES, ...TARGET_PACKAGES, WRAPPER_PACKAGE])
  assert.deepEqual(TARGET_TRIPLES, [
    'aarch64-linux-android',
    'x86_64-linux-android',
    'aarch64-apple-ios',
    'aarch64-apple-ios-sim',
    'wasm32-wasip2',
    'wasm32-wasip1',
  ])
  assert.deepEqual(TARGET_PACKAGES, TARGET_TRIPLES.map(t => `@perryts/perry-target-${t}`))
  assert.equal(npmPackageDir(targetPackageName('wasm32-wasip2')), 'npm/perry-target-wasm32-wasip2')
  assert.equal(ALL_PACKAGES.at(-1), WRAPPER_PACKAGE)
})

test('the wrapper depends on the hosts only, never on a target', () => {
  const template = JSON.parse(
    readFileSync(path.join(REPO_ROOT, 'npm/perry/package.json.tmpl'), 'utf8'),
  ) as { optionalDependencies: Record<string, string>; dependencies?: unknown }
  assert.deepEqual(Object.keys(template.optionalDependencies).sort(), [...PLATFORM_PACKAGES].sort())
  assert.equal(template.dependencies, undefined)
})

test('the size budget covers every target tarball', () => {
  // prepare-ci-packages.mts packs and sizes every ALL_PACKAGES entry.
  const src = readFileSync(path.join(REPO_ROOT, 'scripts/publish/prepare-ci-packages.mts'), 'utf8')
  assert.match(src, /for \(const name of ALL_PACKAGES\)/)
  assert.match(src, /size > budget/)
  for (const name of TARGET_PACKAGES) assert.ok(ALL_PACKAGES.includes(name))
})

test('stageTarget: package.json has no os/cpu/bin/deps; target.json restates the bundle', () => {
  withTemp(dir => {
    const triple = 'aarch64-linux-android'
    const src = bundle(path.join(dir, 'bundle'), triple, {
      files: ['libperry_runtime.a', 'libperry_runtime.a.gc-effects', 'libperry_stdlib.a', 'libperry_ui_android.a'],
    })
    const pkg = path.join(dir, 'pkg')
    const staged = stageTarget({ bundleDir: src, packageDir: pkg, triple, version: VERSION })

    const packageJson = JSON.parse(readFileSync(path.join(pkg, 'package.json'), 'utf8'))
    assert.equal(packageJson.name, '@perryts/perry-target-aarch64-linux-android')
    assert.equal(packageJson.version, VERSION)
    for (const key of ['os', 'cpu', 'libc', 'bin', 'scripts', 'dependencies', 'optionalDependencies', 'peerDependencies']) {
      assert.equal(packageJson[key], undefined, `package.json must not set ${key}`)
    }
    assert.deepEqual(packageJson.files, ['lib/', 'target.json', 'README.md', 'LICENSE'])
    assert.equal(packageJson.publishConfig.access, 'public')

    const bundleManifest = JSON.parse(readFileSync(path.join(src, 'manifest.json'), 'utf8'))
    const targetJson = JSON.parse(readFileSync(path.join(pkg, 'target.json'), 'utf8'))
    assert.deepEqual(targetJson, staged)
    assert.equal(targetJson.schema_version, 1)
    assert.equal(targetJson.perry_version, VERSION)
    assert.equal(targetJson.source_revision, REVISION)
    assert.equal(targetJson.target_triple, triple)
    assert.deepEqual(
      targetJson.files,
      bundleManifest.files.map((f: { path: string }) => ({ ...f, path: `lib/${f.path}` })),
    )
    for (const f of bundleManifest.files) assert.ok(existsSync(path.join(pkg, 'lib', f.path)))
    assert.match(readFileSync(path.join(pkg, 'README.md'), 'utf8'), /@perryts\/perry-target-aarch64-linux-android@0\.5\.1700/)
  })
})

test('stageTarget: the revision comes from the bundle or the release, and must agree', () => {
  withTemp(dir => {
    const triple = 'wasm32-wasip1'
    const pkg = path.join(dir, 'pkg')
    const old = bundle(path.join(dir, 'old'), triple, { revision: '' })
    assert.throws(() => stageTarget({ bundleDir: old, packageDir: pkg, triple, version: VERSION }), /PERRY_SOURCE_REVISION/)
    assert.equal(
      stageTarget({ bundleDir: old, packageDir: pkg, triple, version: VERSION, revision: REVISION }).source_revision,
      REVISION,
    )
    const pinned = bundle(path.join(dir, 'pinned'), triple)
    assert.throws(
      () => stageTarget({ bundleDir: pinned, packageDir: pkg, triple, version: VERSION, revision: 'b'.repeat(40) }),
      /built from a{40}/,
    )
  })
})

test('stageTarget: a tampered, missing, unlisted or foreign bundle file is refused', () => {
  withTemp(dir => {
    const triple = 'aarch64-apple-ios'
    const pkg = path.join(dir, 'pkg')
    const cases: Array<[string, (src: string) => void, RegExp]> = [
      ['sha', src => writeFileSync(path.join(src, 'libperry_runtime.a'), 'tampered'), /the manifest says/],
      ['missing', src => rmSync(path.join(src, 'libperry_runtime.a')), /listed but missing/],
      ['unlisted', src => writeFileSync(path.join(src, 'extra.a'), 'x'), /not in its manifest/],
      ['triple', src => {
        const m = JSON.parse(readFileSync(path.join(src, 'manifest.json'), 'utf8'))
        m.target_triple = 'aarch64-apple-ios-sim'
        writeFileSync(path.join(src, 'manifest.json'), JSON.stringify(m))
      }, /target_triple is aarch64-apple-ios-sim/],
      ['version', src => {
        const m = JSON.parse(readFileSync(path.join(src, 'manifest.json'), 'utf8'))
        m.perry_version = '0.5.1699'
        writeFileSync(path.join(src, 'manifest.json'), JSON.stringify(m))
      }, /perry_version is 0\.5\.1699/],
    ]
    for (const [label, mutate, error] of cases) {
      const src = bundle(path.join(dir, label), triple)
      mutate(src)
      assert.throws(() => stageTarget({ bundleDir: src, packageDir: pkg, triple, version: VERSION }), error, label)
    }
  })
})

// --- stage-npm.sh end to end on a fixture repo ------------------------------
// The script derives its repo root from its own path, so a copy of it with the
// templates and a Cargo.toml stages into the fixture, never into this tree.

const PLATFORM_ARTIFACTS: Array<[string, 'unix' | 'win']> = [
  ['perry-macos-aarch64', 'unix'],
  ['perry-macos-x86_64', 'unix'],
  ['perry-linux-x86_64', 'unix'],
  ['perry-linux-aarch64', 'unix'],
  ['perry-linux-x86_64-musl', 'unix'],
  ['perry-linux-aarch64-musl', 'unix'],
  ['perry-windows-x86_64', 'win'],
  ['perry-windows-aarch64', 'win'],
]

function fixtureRepo(dir: string): { repo: string; artifacts: string } {
  const repo = path.join(dir, 'repo')
  mkdirSync(path.join(repo, 'scripts/publish'), { recursive: true })
  cpSync(path.join(REPO_ROOT, 'scripts/stage-npm.sh'), path.join(repo, 'scripts/stage-npm.sh'))
  for (const file of ['constants.mts', 'stage-target.mts']) {
    cpSync(path.join(REPO_ROOT, 'scripts/publish', file), path.join(repo, 'scripts/publish', file))
  }
  // Templates and the wrapper's launcher only: no staged output of this tree.
  for (const file of [
    'npm/perry/bin/perry.js',
    'npm/perry-target/package.json.tmpl',
    ...[WRAPPER_PACKAGE, ...PLATFORM_PACKAGES].map(name => `${npmPackageDir(name)}/package.json.tmpl`),
  ]) {
    mkdirSync(path.dirname(path.join(repo, file)), { recursive: true })
    cpSync(path.join(REPO_ROOT, file), path.join(repo, file))
  }
  writeFileSync(path.join(repo, 'Cargo.toml'), `[workspace.package]\nversion = "${VERSION}"\n`)
  const artifacts = path.join(dir, 'artifacts')
  for (const [artifact, kind] of PLATFORM_ARTIFACTS) {
    const out = path.join(artifacts, artifact)
    mkdirSync(out, { recursive: true })
    const files = kind === 'win'
      ? ['perry.exe', 'LLVM-C.dll', 'perry_runtime.lib', 'perry_runtime.lib.gc-effects']
      : ['perry', 'libperry_runtime.a', 'libperry_runtime.a.gc-effects']
    for (const f of files) writeFileSync(path.join(out, f), f)
  }
  for (const triple of TARGET_TRIPLES) {
    // The download-artifact layout: perry-cross-<t>/perry-cross-<t>.tar.gz.
    const src = bundle(path.join(dir, 'bundles', triple), triple)
    const out = path.join(artifacts, `perry-cross-${triple}`)
    mkdirSync(out, { recursive: true })
    const tar = spawnSync('tar', ['czf', path.join(out, `perry-cross-${triple}.tar.gz`), '-C', src, '.'])
    assert.equal(tar.status, 0, String(tar.stderr))
  }
  return { repo, artifacts }
}

function stageNpm(repo: string, artifacts: string): { status: number | null; output: string } {
  const run = spawnSync('bash', [path.join(repo, 'scripts/stage-npm.sh'), artifacts], {
    encoding: 'utf8',
    env: { ...process.env, PERRY_NPM_NO_COMPRESS: '1', SKIP_MISSING: '', PERRY_SOURCE_REVISION: REVISION },
  })
  return { status: run.status, output: `${run.stdout}${run.stderr}` }
}

test('stage-npm.sh: stages every target package from its bundle', () => {
  withTemp(dir => {
    const { repo, artifacts } = fixtureRepo(dir)
    const run = stageNpm(repo, artifacts)
    assert.equal(run.status, 0, run.output)
    for (const triple of TARGET_TRIPLES) {
      const pkg = path.join(repo, 'npm', `perry-target-${triple}`)
      const targetJson = JSON.parse(readFileSync(path.join(pkg, 'target.json'), 'utf8'))
      assert.equal(targetJson.target_triple, triple)
      assert.ok(existsSync(path.join(pkg, 'lib/libperry_runtime.a')))
      assert.equal(JSON.parse(readFileSync(path.join(pkg, 'package.json'), 'utf8')).name, targetPackageName(triple))
    }
    assert.match(run.output, /and 6 target packages/)
  })
})

test('stage-npm.sh: a missing target bundle fails the stage (no wrapper without its targets)', () => {
  withTemp(dir => {
    const { repo, artifacts } = fixtureRepo(dir)
    rmSync(path.join(artifacts, 'perry-cross-x86_64-linux-android'), { recursive: true })
    const run = stageNpm(repo, artifacts)
    assert.notEqual(run.status, 0)
    assert.match(run.output, /no perry-cross-x86_64-linux-android\.tar\.gz/)
  })
})

test('stage-npm.sh: a bundle file that differs from its manifest fails the stage', () => {
  withTemp(dir => {
    const { repo, artifacts } = fixtureRepo(dir)
    const triple = 'wasm32-wasip2'
    const src = path.join(dir, 'bundles', triple)
    writeFileSync(path.join(src, 'libperry_runtime.a'), 'tampered')
    const out = path.join(artifacts, `perry-cross-${triple}`, `perry-cross-${triple}.tar.gz`)
    assert.equal(spawnSync('tar', ['czf', out, '-C', src, '.']).status, 0)
    const run = stageNpm(repo, artifacts)
    assert.notEqual(run.status, 0)
    assert.match(run.output, /perry-cross-wasm32-wasip2: libperry_runtime\.a is \d+ bytes/)
  })
})
