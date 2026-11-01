/**
 * @file Tests for the per-target and per-host ext npm packages: the publish
 *   order, the staged package shape (stage-target.mts), and stage-npm.sh's
 *   refusal to stage a release with a missing or tampered cross bundle or a
 *   listed host without its ext archives. Run with
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
  EXT_PACKAGES,
  EXT_TRIPLES,
  PLATFORM_PACKAGES,
  REPO_ROOT,
  TARGET_PACKAGES,
  TARGET_TRIPLES,
  WRAPPER_PACKAGE,
  extPackageName,
  npmPackageDir,
  targetPackageName,
} from './constants.mts'
import { stageExt, stageTarget } from './stage-target.mts'

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

test('ALL_PACKAGES: hosts, then ext packages, then targets, then the wrapper', () => {
  assert.deepEqual(ALL_PACKAGES, [...PLATFORM_PACKAGES, ...EXT_PACKAGES, ...TARGET_PACKAGES, WRAPPER_PACKAGE])
  assert.deepEqual(EXT_TRIPLES, [
    'aarch64-apple-darwin',
    'x86_64-apple-darwin',
    'x86_64-unknown-linux-gnu',
    'aarch64-unknown-linux-gnu',
  ])
  assert.deepEqual(EXT_PACKAGES, EXT_TRIPLES.map(t => `@perryts/perry-ext-${t}`))
  assert.equal(
    npmPackageDir(extPackageName('x86_64-unknown-linux-gnu')),
    'npm/perry-ext-x86_64-unknown-linux-gnu',
  )
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

test('the one triple list is the file perry compiles in', () => {
  const list = JSON.parse(readFileSync(path.join(REPO_ROOT, 'npm/packaged-triples.json'), 'utf8'))
  assert.deepEqual(list.targets, TARGET_TRIPLES)
  assert.deepEqual(list.ext, EXT_TRIPLES)
  const rust = readFileSync(
    path.join(REPO_ROOT, 'crates/perry/src/commands/compile/target_packages.rs'),
    'utf8',
  )
  assert.match(rust, /include_str!\("\.\.\/\.\.\/\.\.\/\.\.\/\.\.\/npm\/packaged-triples\.json"\)/)
  for (const flag of ['--print-targets', '--print-ext']) {
    const run = spawnSync(process.execPath, [path.join(REPO_ROOT, 'scripts/publish/constants.mts'), flag], {
      encoding: 'utf8',
    })
    assert.equal(run.status, 0, run.stderr)
    assert.deepEqual(run.stdout.trim().split('\n'), flag === '--print-ext' ? EXT_TRIPLES : TARGET_TRIPLES)
  }
})

test('the wrapper depends on the hosts only, never on a target or ext package', () => {
  const template = JSON.parse(
    readFileSync(path.join(REPO_ROOT, 'npm/perry/package.json.tmpl'), 'utf8'),
  ) as { optionalDependencies: Record<string, string>; dependencies?: unknown }
  assert.deepEqual(Object.keys(template.optionalDependencies).sort(), [...PLATFORM_PACKAGES].sort())
  assert.equal(template.dependencies, undefined)
})

test('the size budget covers every target and ext tarball', () => {
  // prepare-ci-packages.mts packs and sizes every ALL_PACKAGES entry.
  const src = readFileSync(path.join(REPO_ROOT, 'scripts/publish/prepare-ci-packages.mts'), 'utf8')
  assert.match(src, /for \(const name of ALL_PACKAGES\)/)
  assert.match(src, /size > budget/)
  for (const name of [...TARGET_PACKAGES, ...EXT_PACKAGES]) assert.ok(ALL_PACKAGES.includes(name))
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

test('stageExt: every ext archive of the release archive, hashed into target.json', () => {
  withTemp(dir => {
    const triple = 'x86_64-unknown-linux-gnu'
    const src = path.join(dir, 'release')
    mkdirSync(src)
    for (const name of ['perry', 'libperry_runtime.a', 'libperry_ext_http.a', 'libperry_ext_ws.a']) {
      writeFileSync(path.join(src, name), `${name} bytes`)
    }
    const pkg = path.join(dir, 'pkg')
    assert.throws(() => stageExt({ sourceDir: src, packageDir: pkg, triple, version: VERSION }), /PERRY_SOURCE_REVISION/)
    const staged = stageExt({ sourceDir: src, packageDir: pkg, triple, version: VERSION, revision: REVISION })
    const packageJson = JSON.parse(readFileSync(path.join(pkg, 'package.json'), 'utf8'))
    assert.equal(packageJson.name, '@perryts/perry-ext-x86_64-unknown-linux-gnu')
    for (const key of ['os', 'cpu', 'libc', 'bin', 'scripts', 'dependencies', 'optionalDependencies', 'peerDependencies']) {
      assert.equal(packageJson[key], undefined, `package.json must not set ${key}`)
    }
    assert.deepEqual(packageJson.files, ['lib/', 'target.json', 'README.md', 'LICENSE'])
    const targetJson = JSON.parse(readFileSync(path.join(pkg, 'target.json'), 'utf8'))
    assert.deepEqual(targetJson, staged)
    assert.equal(targetJson.target_triple, triple)
    assert.equal(targetJson.source_revision, REVISION)
    assert.deepEqual(
      targetJson.files,
      ['libperry_ext_http.a', 'libperry_ext_ws.a'].map(name => ({
        path: `lib/${name}`,
        sha256: sha256(`${name} bytes`),
        size: Buffer.byteLength(`${name} bytes`),
      })),
    )
    assert.ok(!existsSync(path.join(pkg, 'lib/libperry_runtime.a')), 'core archives stay in the host package')
    assert.match(readFileSync(path.join(pkg, 'README.md'), 'utf8'), /perry target add --ext/)
    rmSync(path.join(src, 'libperry_ext_http.a'))
    rmSync(path.join(src, 'libperry_ext_ws.a'))
    assert.throws(
      () => stageExt({ sourceDir: src, packageDir: pkg, triple, version: VERSION, revision: REVISION }),
      /has no libperry_ext_\* archives/,
    )
  })
})

// --- stage-npm.sh end to end on a fixture repo ------------------------------
// The script derives its repo root from its own path, so a copy of it with the
// templates and a Cargo.toml stages into the fixture, never into this tree.

// The kind, and whether the release archive carries ext archives (the
// release legs build them for macOS and Linux glibc only).
const PLATFORM_ARTIFACTS: Array<[string, 'unix' | 'win', boolean]> = [
  ['perry-macos-aarch64', 'unix', true],
  ['perry-macos-x86_64', 'unix', true],
  ['perry-linux-x86_64', 'unix', true],
  ['perry-linux-aarch64', 'unix', true],
  ['perry-linux-x86_64-musl', 'unix', false],
  ['perry-linux-aarch64-musl', 'unix', false],
  ['perry-windows-x86_64', 'win', false],
  ['perry-windows-aarch64', 'win', false],
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
    'npm/packaged-triples.json',
    'npm/perry/bin/perry.js',
    'npm/perry-target/package.json.tmpl',
    'npm/perry-ext/package.json.tmpl',
    ...[WRAPPER_PACKAGE, ...PLATFORM_PACKAGES].map(name => `${npmPackageDir(name)}/package.json.tmpl`),
  ]) {
    mkdirSync(path.dirname(path.join(repo, file)), { recursive: true })
    cpSync(path.join(REPO_ROOT, file), path.join(repo, file))
  }
  writeFileSync(path.join(repo, 'Cargo.toml'), `[workspace.package]\nversion = "${VERSION}"\n`)
  const artifacts = path.join(dir, 'artifacts')
  for (const [artifact, kind, ext] of PLATFORM_ARTIFACTS) {
    const out = path.join(artifacts, artifact)
    mkdirSync(out, { recursive: true })
    const files = kind === 'win'
      ? ['perry.exe', 'LLVM-C.dll', 'perry_runtime.lib', 'perry_runtime.lib.gc-effects']
      : ['perry', 'libperry_runtime.a', 'libperry_runtime.a.gc-effects']
    if (ext) files.push('libperry_ext_http.a', 'libperry_ext_zlib.a')
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
    assert.match(run.output, /4 ext packages and 6 target packages/)
    for (const triple of EXT_TRIPLES) {
      const pkg = path.join(repo, 'npm', `perry-ext-${triple}`)
      const targetJson = JSON.parse(readFileSync(path.join(pkg, 'target.json'), 'utf8'))
      assert.equal(targetJson.target_triple, triple)
      assert.deepEqual(targetJson.files.map((f: { path: string }) => f.path), [
        'lib/libperry_ext_http.a',
        'lib/libperry_ext_zlib.a',
      ])
      assert.equal(JSON.parse(readFileSync(path.join(pkg, 'package.json'), 'utf8')).name, extPackageName(triple))
    }
    // The host packages keep no ext archive; musl and Windows get no ext package.
    assert.ok(!existsSync(path.join(repo, 'npm/perry-linux-x64/lib/libperry_ext_http.a')))
    assert.ok(!existsSync(path.join(repo, 'npm/perry-ext-x86_64-unknown-linux-musl')))
  })
})

test('stage-npm.sh: a listed ext host whose release archive has no ext archives fails the stage', () => {
  withTemp(dir => {
    const { repo, artifacts } = fixtureRepo(dir)
    rmSync(path.join(artifacts, 'perry-linux-aarch64', 'libperry_ext_http.a'))
    rmSync(path.join(artifacts, 'perry-linux-aarch64', 'libperry_ext_zlib.a'))
    const run = stageNpm(repo, artifacts)
    assert.notEqual(run.status, 0)
    assert.match(run.output, /perry-ext-aarch64-unknown-linux-gnu: .* has no libperry_ext_\* archives/)
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
