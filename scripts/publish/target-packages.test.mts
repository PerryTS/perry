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
  readdirSync,
  readFileSync,
  rmSync,
  appendFileSync,
  statSync,
  writeFileSync,
} from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { execFileSync, spawnSync } from 'node:child_process'

import {
  ALL_PACKAGES,
  EXT_PACKAGES,
  UI_PACKAGES,
  WASM_HOST_PACKAGES,
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
import { archiveKind, stageHost } from './archive-kind.mts'
import { isStrippableHostArchive, isStrippableUiArchive, stageExt, stageTarget, stageKind } from './stage-target.mts'

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
  assert.deepEqual(ALL_PACKAGES, [...PLATFORM_PACKAGES, ...EXT_PACKAGES, ...UI_PACKAGES, ...WASM_HOST_PACKAGES, ...TARGET_PACKAGES, WRAPPER_PACKAGE])
  assert.deepEqual(EXT_TRIPLES, [
    'aarch64-apple-darwin',
    'x86_64-apple-darwin',
    'x86_64-unknown-linux-gnu',
    'aarch64-unknown-linux-gnu',
    'x86_64-unknown-linux-musl',
    'aarch64-unknown-linux-musl',
    'x86_64-pc-windows-msvc',
    'aarch64-pc-windows-msvc',
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
  for (const name of [...TARGET_PACKAGES, ...EXT_PACKAGES, ...UI_PACKAGES, ...WASM_HOST_PACKAGES]) assert.ok(ALL_PACKAGES.includes(name))
})

test('stageTarget: package.json has no os/cpu/bin/deps; target.json restates the bundle', () => {
  withTemp(dir => {
    const triple = 'aarch64-linux-android'
    const src = bundle(path.join(dir, 'bundle'), triple, {
      files: ['libperry_runtime.a', 'libperry_runtime.a.gc-effects', 'libperry_stdlib.a', 'libperry_ui_android.a'],
    })
    const pkg = path.join(dir, 'pkg')
    const stripped: string[] = []
    const staged = stageTarget({
      bundleDir: src,
      packageDir: pkg,
      triple,
      version: VERSION,
      stripArchive: archive => stripped.push(path.basename(archive)),
    })
    assert.deepEqual(stripped, ['libperry_ui_android.a'], 'only the UI archive is stripped')

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

test('isStrippableHostArchive: ext archives of every host', () => {
  assert.equal(isStrippableHostArchive('x86_64-unknown-linux-musl', 'libperry_ext_http.a'), true)
  assert.equal(isStrippableHostArchive('x86_64-pc-windows-msvc', 'perry_ext_http.lib'), true)
  assert.equal(isStrippableHostArchive('aarch64-apple-darwin', 'libperry_ext_http.a'), true)
  assert.equal(isStrippableHostArchive('x86_64-unknown-linux-gnu', 'libperry_runtime.a'), false)
  // Every optional host kind is stripped before its manifest is hashed.
  assert.equal(isStrippableHostArchive('x86_64-unknown-linux-gnu', 'libperry_ui_gtk4.a'), true)
  assert.equal(isStrippableHostArchive('x86_64-pc-windows-msvc', 'perry_ui_windows.lib'), true)
  assert.equal(isStrippableHostArchive('x86_64-pc-windows-msvc', 'perry_wasm_host.lib'), true)
  assert.equal(isStrippableHostArchive('aarch64-unknown-linux-gnu', 'libperry_wasm_host.a'), true)
  assert.equal(isStrippableHostArchive('aarch64-apple-darwin', 'libperry_ui_macos.a'), true)
  assert.equal(isStrippableHostArchive('aarch64-apple-darwin', 'libperry_wasm_host.a'), true)
  assert.equal(isStrippableHostArchive('x86_64-unknown-linux-gnu', 'libperry_stdlib.a'), false)
})

test('stageExt: every ext archive of the release archive, hashed into target.json', () => {
  withTemp(dir => {
    const triple = 'x86_64-unknown-linux-gnu'
    const src = path.join(dir, 'release')
    mkdirSync(src)
    const shipped = ['libperry_ext_http.a', 'libperry_ext_ws.a']
    for (const name of ['perry', 'libperry_runtime.a', 'libperry_stdlib.a', 'libperry_ui_gtk4.a', 'libperry_wasm_host.a', ...shipped]) {
      writeFileSync(path.join(src, name), `${name} bytes`)
    }
    const pkg = path.join(dir, 'pkg')
    assert.throws(() => stageExt({ sourceDir: src, packageDir: pkg, triple, version: VERSION }), /PERRY_SOURCE_REVISION/)
    // The stub marks every archive it is handed; target.json must describe the marked copy.
    const stripArchive = (archive: string) => appendFileSync(archive, '!')
    const staged = stageExt({ sourceDir: src, packageDir: pkg, triple, version: VERSION, revision: REVISION, stripArchive })
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
      [...shipped].sort().map(name => ({
        path: `lib/${name}`,
        sha256: sha256(`${name} bytes!`),
        size: Buffer.byteLength(`${name} bytes!`),
      })),
    )
    assert.equal(readFileSync(path.join(src, 'libperry_ext_http.a'), 'utf8'), 'libperry_ext_http.a bytes', 'the release archive is not touched')
    assert.ok(!existsSync(path.join(pkg, 'lib/libperry_runtime.a')), 'core archives stay in the host package')
    assert.ok(!existsSync(path.join(pkg, 'lib/libperry_stdlib.a')), 'core archives stay in the host package')
    assert.match(readFileSync(path.join(pkg, 'README.md'), 'utf8'), /perry target add --ext/)
    rmSync(path.join(src, 'libperry_ext_http.a'))
    rmSync(path.join(src, 'libperry_ext_ws.a'))
    // UI and wasm_host without any ext archive: a broken release archive.
    assert.throws(
      () => stageExt({ sourceDir: src, packageDir: pkg, triple, version: VERSION, revision: REVISION }),
      /has no ext archives/,
    )
  })
})

// --- stage-npm.sh end to end on a fixture repo ------------------------------
// The script derives its repo root from its own path, so a copy of it with the
// templates and a Cargo.toml stages into the fixture, never into this tree.

// The kind, and whether the release archive carries ext archives (every
// release leg builds them: libperry_ext_*.a on Unix, perry_ext_*.lib on Windows).
// The last column is the host triple (stage-npm.sh's PLATFORMS).
const PLATFORM_ARTIFACTS: Array<[string, 'unix' | 'win', boolean, string]> = [
  ['perry-macos-aarch64', 'unix', true, 'aarch64-apple-darwin'],
  ['perry-macos-x86_64', 'unix', true, 'x86_64-apple-darwin'],
  ['perry-linux-x86_64', 'unix', true, 'x86_64-unknown-linux-gnu'],
  ['perry-linux-aarch64', 'unix', true, 'aarch64-unknown-linux-gnu'],
  ['perry-linux-x86_64-musl', 'unix', true, 'x86_64-unknown-linux-musl'],
  ['perry-linux-aarch64-musl', 'unix', true, 'aarch64-unknown-linux-musl'],
  ['perry-windows-x86_64', 'win', true, 'x86_64-pc-windows-msvc'],
  ['perry-windows-aarch64', 'win', true, 'aarch64-pc-windows-msvc'],
]

/** The optional archives in each build artifact (no UI on musl). */
function optionalArchives(artifact: string, kind: 'unix' | 'win'): string[] {
  if (kind === 'win') return ['perry_ui_windows.lib', 'perry_wasm_host.lib']
  if (artifact.startsWith('perry-macos-')) return ['libperry_ui_macos.a', 'libperry_wasm_host.a']
  if (artifact.endsWith('-musl')) return ['libperry_wasm_host.a']
  return ['libperry_ui_gtk4.a', 'libperry_wasm_host.a']
}

function fixtureRepo(dir: string): { repo: string; artifacts: string } {
  const repo = path.join(dir, 'repo')
  mkdirSync(path.join(repo, 'scripts/publish'), { recursive: true })
  cpSync(path.join(REPO_ROOT, 'scripts/stage-npm.sh'), path.join(repo, 'scripts/stage-npm.sh'))
  for (const file of ['constants.mts', 'stage-target.mts', 'archive-kind.mts']) {
    cpSync(path.join(REPO_ROOT, 'scripts/publish', file), path.join(repo, 'scripts/publish', file))
  }
  // The fixture archives are not real ar files: a no-op stand-in for the strip step
  // (its own test below runs the real script on real archives).
  writeFileSync(path.join(repo, 'scripts/strip-archive-members.py'), '#!/usr/bin/env python3\n')
  // Templates and the wrapper's launcher only: no staged output of this tree.
  for (const file of [
    'npm/packaged-triples.json',
    'npm/archive-kinds.rules',
    'npm/perry-ui/package.json.tmpl',
    'npm/perry-wasm-host/package.json.tmpl',
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
      ? ['perry.exe', 'LLVM-C.dll', 'perry_runtime.lib', 'perry_runtime.lib.gc-effects', 'perry_stdlib.lib']
      : ['perry', 'libperry_runtime.a', 'libperry_runtime.a.gc-effects', 'libperry_stdlib.a']
    files.push(...optionalArchives(artifact, kind))
    if (ext) {
      files.push(...(kind === 'win' ? ['perry_ext_http.lib', 'perry_ext_zlib.lib'] : ['libperry_ext_http.a', 'libperry_ext_zlib.a']))
    }
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
    assert.match(run.output, /8 ext packages and 6 target packages/)
    for (const triple of EXT_TRIPLES) {
      const pkg = path.join(repo, 'npm', `perry-ext-${triple}`)
      const targetJson = JSON.parse(readFileSync(path.join(pkg, 'target.json'), 'utf8'))
      assert.equal(targetJson.target_triple, triple)
      const artifact = PLATFORM_ARTIFACTS.find(row => row[3] === triple)
      assert.ok(artifact, triple)
      const ext = triple.includes('windows')
        ? ['perry_ext_http.lib', 'perry_ext_zlib.lib']
        : ['libperry_ext_http.a', 'libperry_ext_zlib.a']
      assert.deepEqual(
        targetJson.files.map((f: { path: string }) => f.path),
        [...ext].sort().map(name => `lib/${name}`),
      )
      assert.equal(JSON.parse(readFileSync(path.join(pkg, 'package.json'), 'utf8')).name, extPackageName(triple))
      for (const name of optionalArchives(artifact[0], artifact[1])) {
        const kind = archiveKind(name)
        const dir = path.join(repo, 'npm', `perry-${kind}-${triple}`)
        const manifest = JSON.parse(readFileSync(path.join(dir, 'target.json'), 'utf8'))
        assert.deepEqual(manifest.files.map((f: {path: string}) => f.path), [`lib/${name}`])
        const owners = ['ext', 'ui', 'wasm-host'].filter(k => existsSync(path.join(repo, 'npm', `perry-${k}-${triple}`, 'lib', name)))
        assert.deepEqual(owners, [kind], `${triple}/${name} has exactly one owner`)
      }
    }
    // The host packages keep the runtime and stdlib, and no ext, UI or wasm_host archive.
    for (const [suffix, kind] of [['linux-x64', 'unix'], ['linux-x64-musl', 'unix'], ['darwin-arm64', 'unix'], ['win32-x64', 'win']] as const) {
      const lib = path.join(repo, 'npm', `perry-${suffix}`, 'lib')
      const names = readdirSync(lib).sort()
      assert.deepEqual(
        names,
        kind === 'win'
          ? ['perry_runtime.lib', 'perry_runtime.lib.gc-effects', 'perry_stdlib.lib']
          : ['libperry_runtime.a', 'libperry_runtime.a.gc-effects', 'libperry_stdlib.a'],
        suffix,
      )
    }
  })
})

test('stage-npm.sh: a listed ext host whose release archive has no ext archives fails the stage', () => {
  withTemp(dir => {
    const { repo, artifacts } = fixtureRepo(dir)
    rmSync(path.join(artifacts, 'perry-linux-aarch64', 'libperry_ext_http.a'))
    rmSync(path.join(artifacts, 'perry-linux-aarch64', 'libperry_ext_zlib.a'))
    const run = stageNpm(repo, artifacts)
    assert.notEqual(run.status, 0)
    assert.match(run.output, /perry-ext-aarch64-unknown-linux-gnu: .* has no ext archives/)
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

test('isStrippableUiArchive: UI archives of non-Apple targets only', () => {
  assert.equal(isStrippableUiArchive('aarch64-linux-android', 'libperry_ui_android.a'), true)
  assert.equal(isStrippableUiArchive('x86_64-pc-windows-msvc', 'perry_ui_windows.lib'), true)
  assert.equal(isStrippableUiArchive('aarch64-apple-ios', 'libperry_ui_ios.a'), false)
  assert.equal(isStrippableUiArchive('aarch64-linux-android', 'libperry_runtime.a'), false)
  assert.equal(isStrippableUiArchive('aarch64-linux-android', 'libperry_ui_android.a.gc-effects'), false)
})

/** A tool from PATH or /usr/lib/llvm-*, or undefined. */
function findTool(name: string): string | undefined {
  const dirs = (process.env.PATH ?? '').split(path.delimiter)
  try {
    for (const d of readdirSync('/usr/lib')) if (d.startsWith('llvm-')) dirs.push(`/usr/lib/${d}/bin`)
  } catch {}
  return dirs.map(d => path.join(d, name)).find(f => existsSync(f))
}

test('stageTarget: the UI archive is stripped before its hash is recorded (real strip)', t => {
  const objcopy = findTool('llvm-objcopy')
  const llvmAr = findTool('llvm-ar')
  const cc = findTool('cc') ?? findTool('clang')
  if (!objcopy || !llvmAr || !cc) {
    t.skip(`needs llvm-objcopy, llvm-ar and a C compiler (have ${[objcopy, llvmAr, cc].join(',')})`)
    return
  }
  withTemp(dir => {
    const triple = 'aarch64-linux-android'
    const run = (cmd: string, args: string[]) => execFileSync(cmd, args, { cwd: dir, stdio: 'pipe' })
    writeFileSync(path.join(dir, 'a.c'), 'int perry_test_sym(void) { return 7; }\n')
    writeFileSync(path.join(dir, 'bc.bin'), Buffer.alloc(4096, 1))
    run(cc, ['-c', 'a.c', '-o', 'plain.o'])
    run(objcopy, ['--add-section', '.llvmbc=bc.bin', '--add-section', '.llvmcmd=bc.bin', 'plain.o', 'a.o'])
    run(llvmAr, ['rcs', 'libperry_ui_android.a', 'a.o'])
    const src = bundle(path.join(dir, 'bundle'), triple, { files: ['libperry_runtime.a'] })
    // Add the real UI archive to the bundle and its manifest.
    const archive = readFileSync(path.join(dir, 'libperry_ui_android.a'))
    writeFileSync(path.join(src, 'libperry_ui_android.a'), archive)
    const manifest = JSON.parse(readFileSync(path.join(src, 'manifest.json'), 'utf8'))
    manifest.files.push({ path: 'libperry_ui_android.a', sha256: sha256(archive), size: archive.length })
    writeFileSync(path.join(src, 'manifest.json'), JSON.stringify(manifest))

    const pkg = path.join(dir, 'pkg')
    const staged = stageTarget({ bundleDir: src, packageDir: pkg, triple, version: VERSION })

    const out = path.join(pkg, 'lib', 'libperry_ui_android.a')
    const entry = staged.files.find(f => f.path === 'lib/libperry_ui_android.a')!
    assert.ok(entry.size < archive.length, 'the staged UI archive is smaller')
    assert.equal(entry.size, statSync(out).size)
    assert.equal(entry.sha256, sha256(readFileSync(out)), 'target.json hashes the stripped archive')
    assert.notEqual(entry.sha256, sha256(archive))
    const sections = execFileSync(findTool('llvm-objdump') ?? 'objdump', ['-h', out], { encoding: 'utf8' })
    assert.doesNotMatch(sections, /\.llvmbc|\.llvmcmd/)
    assert.match(execFileSync(findTool('llvm-nm') ?? 'nm', [out], { encoding: 'utf8' }), /perry_test_sym/)
    // The runtime archive is not touched.
    assert.deepEqual(readFileSync(path.join(pkg, 'lib', 'libperry_runtime.a')), readFileSync(path.join(src, 'libperry_runtime.a')))
    // The extracted bundle itself is left as built.
    assert.equal(sha256(readFileSync(path.join(src, 'libperry_ui_android.a'))), sha256(archive))
  })
})


test('one classifier assigns host, UI, wasm-host, ext and cross archives', () => {
  const cases = {
    'libperry_runtime.a': 'host', 'perry_stdlib.lib': 'host',
    'libperry_runtime.a.gc-effects': 'host',
    'libperry_ui_macos.a': 'ui', 'perry_ui_windows.lib': 'ui',
    'libperry_ui_gtk4.a.zst': 'ui',
    'libperry_wasm_host.a': 'wasm-host', 'perry_wasm_host.lib': 'wasm-host',
    'libperry_ext_bcrypt.a': 'ext', 'perry_ext_http.lib': 'ext',
    'libperry_runtime_ios.a': 'target', 'libperry_ui_ios_sim.a': 'target',
  }
  for (const [name, kind] of Object.entries(cases)) assert.equal(archiveKind(name), kind, name)
  withTemp(dir => {
    const source = path.join(dir, 'source'), dest = path.join(dir, 'host')
    mkdirSync(source)
    for (const name of ['perry', 'LLVM-C.dll', 'xwin.exe', ...Object.keys(cases)]) writeFileSync(path.join(source, name), name)
    stageHost(source, dest)
    assert.deepEqual(readdirSync(dest).sort(), ['LLVM-C.dll', 'libperry_runtime.a', 'libperry_runtime.a.gc-effects', 'perry', 'perry_stdlib.lib'])
  })
})
