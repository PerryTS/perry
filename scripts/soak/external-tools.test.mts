import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { test } from 'node:test'
import { fileURLToPath } from 'node:url'

import { SOAK_DAYS, addDaysIso, todayIso } from './constants.mts'
import {
  checkDockerPrebake,
  checkPins,
  download,
  extractArchive,
  installTool,
  main,
  pruneExpiredSoakBypasses,
  staleBypasses,
} from './external-tools.mts'
import { DOCKER_PREBAKE, EXTERNAL_TOOLS_JSON, REPO_ROOT, SURFACES } from './paths.mts'

const GOOD_SRI =
  'sha512-waLrsPG2a7EOv0XuvXDQZGgCZ4MTtOfZh8TmGbM6gn2B6Nh6HI+15jaoKdAS9wgdTyIqTuqU+O+NtVYd+kuFaA=='

test('the repo external-tools.json passes checkPins', () => {
  const tools = JSON.parse(readFileSync(EXTERNAL_TOOLS_JSON, 'utf8')).tools
  assert.deepEqual(checkPins(tools), [])
})

test('checkPins flags missing pins, bad SRIs, and asset entries with no integrity', () => {
  const old = addDaysIso(todayIso(), -SOAK_DAYS)
  assert.ok(checkPins({ a: {} }).some(problem => /no version or purl/.test(problem)))
  assert.equal(
    checkPins({ a: { version: '1.0.0', published: old, integrity: 'sha256-abc' } }).length,
    1,
  )
  assert.equal(
    checkPins({ a: { version: '1.0.0', published: old, release: 'asset' } }).length,
    1,
  )
})

test('checkPins requires real, non-future publication dates and a bypass while young', () => {
  const old = addDaysIso(todayIso(), -SOAK_DAYS)
  const young = addDaysIso(todayIso(), -1)
  assert.ok(
    checkPins({ a: { version: '1.0.0' } }).some(problem => /no published release date/.test(problem)),
  )
  assert.ok(
    checkPins({ a: { version: '1.0.0', published: addDaysIso(todayIso(), 1) } }).some(problem =>
      /in the future/.test(problem),
    ),
  )
  assert.ok(
    checkPins({ a: { version: '1.0.0', published: young } }).some(problem =>
      /has no soakBypass/.test(problem),
    ),
  )
  assert.deepEqual(checkPins({ a: { version: '1.0.0', published: old } }), [])
})

test('checkPins validates soakBypass dates and arithmetic; expiry is a warning, not a failure', () => {
  const pub = addDaysIso(todayIso(), -1)
  const good = {
    a: {
      version: '1.0.0',
      published: pub,
      integrity: GOOD_SRI,
      soakBypass: { version: '1.0.0', published: pub, removable: addDaysIso(pub, SOAK_DAYS) },
    },
  }
  assert.deepEqual(checkPins(good), [])
  assert.deepEqual(staleBypasses(good), [])
  const wrongMath = structuredClone(good)
  wrongMath.a.soakBypass.removable = addDaysIso(pub, 3)
  assert.match(checkPins(wrongMath)[0]!, /removable/)
  // Expired-but-valid is STALE, not unsafe: checkPins exits clean, the
  // stale list reports it, and --fix / soak-autofix prunes it.
  const expired = structuredClone(good)
  expired.a.published = '2020-01-01'
  expired.a.soakBypass = {
    version: '1.0.0',
    published: expired.a.published,
    removable: addDaysIso(expired.a.published, SOAK_DAYS),
  }
  assert.deepEqual(checkPins(expired), [])
  assert.deepEqual(staleBypasses(expired), ['a'])
  const impossible = structuredClone(good)
  impossible.a.published = '2026-13-45'
  impossible.a.soakBypass = { version: '1.0.0', published: '2026-13-45', removable: '2026-13-52' }
  assert.match(checkPins(impossible)[0]!, /calendar/)
  assert.deepEqual(staleBypasses(impossible), [])
})

test('pruneExpiredSoakBypasses prunes only valid, expired annotations', () => {
  const pub = addDaysIso(todayIso(), -1)
  const doc = {
    tools: {
      fresh: {
        version: '1.0.0',
        integrity: GOOD_SRI,
        soakBypass: { version: '1.0.0', published: pub, removable: addDaysIso(pub, SOAK_DAYS) },
      },
      expired: {
        version: '1.0.0',
        integrity: GOOD_SRI,
        soakBypass: {
          version: '1.0.0',
          published: '2020-01-01',
          removable: addDaysIso('2020-01-01', SOAK_DAYS),
        },
      },
      // Malformed dates stay findings for a human — never silently pruned.
      malformed: {
        version: '1.0.0',
        integrity: GOOD_SRI,
        soakBypass: { version: '1.0.0', published: '2026-13-45', removable: '2026-13-52' },
      },
      unannotated: { version: '1.0.0', integrity: GOOD_SRI },
    },
  }
  // Wrong-arithmetic + already-past removable: hard failure territory,
  // never pruned or stale.
  ;(doc.tools as Record<string, unknown>)['wrongmath'] = {
    version: '1.0.0',
    integrity: GOOD_SRI,
    soakBypass: { version: '1.0.0', published: todayIso(), removable: '2020-01-02' },
  }
  assert.deepEqual(pruneExpiredSoakBypasses(doc), ['expired'])
  assert.ok('soakBypass' in (doc.tools as Record<string, { soakBypass?: object }>)['wrongmath']!)
  assert.deepEqual(staleBypasses(doc.tools), [])
  assert.ok(doc.tools.fresh.soakBypass)
  assert.ok(!('soakBypass' in doc.tools.expired))
  assert.ok(doc.tools.malformed.soakBypass)
  // Idempotent: a second pass finds nothing left to prune.
  assert.deepEqual(pruneExpiredSoakBypasses(doc), [])
})

test('the repo Dockerfile prebake (when present) matches the tracked pins', t => {
  if (!DOCKER_PREBAKE || !existsSync(path.join(REPO_ROOT, DOCKER_PREBAKE))) {
    t.skip('repo has no prebake image')
    return
  }
  const tools = JSON.parse(readFileSync(EXTERNAL_TOOLS_JSON, 'utf8')).tools
  const docker = readFileSync(path.join(REPO_ROOT, DOCKER_PREBAKE), 'utf8')
  const toolchain = SURFACES.toolchainToml
    ? readFileSync(path.join(REPO_ROOT, SURFACES.toolchainToml), 'utf8')
    : ''
  assert.deepEqual(checkDockerPrebake(docker, tools, toolchain), [])
  // and drift in any direction is caught
  assert.ok(checkDockerPrebake(docker.replace(/sha=[0-9a-f]{8}/, 'sha=deadbeef'), tools, toolchain).length > 0)
  assert.ok(
    checkDockerPrebake(docker, tools, toolchain.replace(/channel = ".*"/, 'channel = "nightly-1999-01-01"')).length >
      0,
  )
})

const sriOf = (buf: Buffer) => `sha512-${createHash('sha512').update(buf).digest('base64')}`

function withEnv(name: string, value: string | undefined, fn: () => Promise<void>): Promise<void> {
  const saved = process.env[name]
  if (value === undefined) {
    delete process.env[name]
  } else {
    process.env[name] = value
  }
  return fn().finally(() => {
    if (saved === undefined) {
      delete process.env[name]
    } else {
      process.env[name] = saved
    }
  })
}

// Hermetic fixture: one line per drift class, so every checkDockerPrebake
// failure branch is exercised even in a repo with no prebake image.
test('checkDockerPrebake flags every drift class (synthetic image)', () => {
  const tools = {
    'sfw-free': {
      version: '1.0.0',
      platforms: { 'linux-arm64': { asset: 'sfw-linux-arm64', integrity: GOOD_SRI } },
    },
  }
  const wrongHex = 'ab'.repeat(64)
  const body = [
    'for cmd in npm yarn; do make_shim "$cmd"; done',
    'RUN curl -o /x https://github.com/SocketDev/sfw-free/releases/download/v0.9.9/sfw-linux-arm64',
    'COPY rack/sfw-free/0.9.9/sfw /usr/local/bin/sfw',
    `RUN asset=ghost-asset; sha=${wrongHex} verify`,
    `RUN asset=sfw-linux-arm64; sha=${wrongHex} verify`,
  ].join('\n')
  const problems = checkDockerPrebake(body, tools, 'channel = "nightly-2026-07-04"\n', '9.9.9')
  for (const needle of [
    /shim list/,
    /msrv toolchain/,
    /version 0\.9\.9/,
    /download url v0\.9\.9/,
    /ghost-asset has no pin/,
    /sha for sfw-linux-arm64/,
    /pinned toolchain nightly-2026-07-04/,
  ]) {
    assert.ok(problems.some(p => needle.test(p)), `expected a finding matching ${needle}`)
  }
  assert.ok(
    checkDockerPrebake('nothing pinned here', tools, '').some(p =>
      /no asset\/sha pin pairs/.test(p),
    ),
  )
})

// The reason the arg-list parse replaced a substring match: a multi-arg
// install line must SATISFY an msrv it contains. Only the negative case was
// covered before, so the fix itself was untested.
test('checkDockerPrebake accepts an msrv satisfied by a later install arg', () => {
  const tools = {
    'sfw-free': {
      version: '1.0.0',
      platforms: { 'linux-arm64': { asset: 'sfw-linux-arm64', integrity: GOOD_SRI } },
    },
  }
  const body = [
    'for cmd in npm yarn pnpm pip pip3 uv cargo; do make_shim "$cmd"; done',
    'RUN rustup toolchain install 1.91.0 1.93.0 --profile minimal',
    'RUN curl -o /x https://github.com/SocketDev/sfw-free/releases/download/v1.0.0/sfw-linux-arm64',
    'COPY rack/sfw-free/1.0.0/sfw /usr/local/bin/sfw',
    `RUN asset=sfw-linux-arm64; sha=${'0'.repeat(128)} verify`,
  ].join('\n')
  // 1.93 is the SECOND argument — a substring match for
  // "toolchain install 1.93" would miss it and false-fail.
  const problems = checkDockerPrebake(body, tools, '', '1.93')
  assert.equal(problems.some(p => /msrv toolchain/.test(p)), false)
  // And a version the line does NOT install is still reported.
  assert.ok(checkDockerPrebake(body, tools, '', '1.99').some(p => /msrv toolchain/.test(p)))
})

// The 5xx branch is the one this retry logic is named for; the auth
// fallback test above does not reach it.
test('download retries a 5xx and succeeds on the second attempt', async t => {
  const payload = Buffer.from('after-5xx')
  let calls = 0
  t.mock.method(globalThis, 'fetch', async () => {
    calls += 1
    return calls === 1 ? new Response('boom', { status: 503 }) : new Response(payload)
  })
  await withEnv('GITHUB_TOKEN', undefined, async () => {
    assert.deepEqual(
      await download('https://example.com/a', sriOf(payload)),
      payload,
    )
  })
  assert.equal(calls, 2)
})

test('download sends the GitHub token to github.com only', async t => {
  const payload = Buffer.from('pinned-bytes')
  const seen: Array<{ host: string; auth: string | undefined }> = []
  t.mock.method(globalThis, 'fetch', async (url: string | URL, init?: RequestInit) => {
    seen.push({
      host: new URL(String(url)).hostname,
      auth: (init?.headers as Record<string, string> | undefined)?.authorization,
    })
    return new Response(payload)
  })
  await withEnv('GITHUB_TOKEN', 'ghs_test_token', async () => {
    await download('https://github.com/o/r/releases/download/v1/a.tgz', sriOf(payload))
    await download('https://registry.npmjs.org/x/-/x-1.0.0.tgz', sriOf(payload))
  })
  assert.equal(seen[0]!.auth, 'Bearer ghs_test_token')
  assert.equal(seen[1]!.auth, undefined)
})

test('download keeps auth across a 5xx retry, drops it only on 401/403/404', async t => {
  const payload = Buffer.from('private-bytes')
  const seen: Array<string | undefined> = []
  let calls = 0
  t.mock.method(globalThis, 'fetch', async (_url: string | URL, init?: RequestInit) => {
    seen.push((init?.headers as Record<string, string> | undefined)?.authorization)
    calls += 1
    // First attempt: transient 500. Retry must still carry the token, or a
    // private asset would 404 and never recover.
    return calls === 1 ? new Response('boom', { status: 500 }) : new Response(payload)
  })
  await withEnv('GITHUB_TOKEN', 'ghs_test_token', async () => {
    const got = await download('https://github.com/o/r/releases/download/v1/a', sriOf(payload))
    assert.deepEqual(got, payload)
  })
  assert.equal(seen.length, 2)
  assert.ok(seen[0], 'first attempt is authed')
  assert.ok(seen[1], 'the 5xx retry stays authed')
})

test('download falls back to unauthenticated when the authed fetch fails', async t => {
  const payload = Buffer.from('public-bytes')
  const seen: Array<string | undefined> = []
  t.mock.method(globalThis, 'fetch', async (_url: string | URL, init?: RequestInit) => {
    const auth = (init?.headers as Record<string, string> | undefined)?.authorization
    seen.push(auth)
    // Authed fetch is rejected 404 (as a public cross-repo asset endpoint
    // can when handed an Actions token); the unauthenticated retry wins.
    return auth ? new Response('nope', { status: 404 }) : new Response(payload)
  })
  await withEnv('GITHUB_TOKEN', 'ghs_test_token', async () => {
    const got = await download('https://github.com/o/r/releases/download/v1/a', sriOf(payload))
    assert.deepEqual(got, payload)
  })
  assert.equal(seen.length, 2)
  assert.ok(seen[0])
  assert.equal(seen[1], undefined)
})

test('download rejects http errors and integrity mismatches', async t => {
  const payload = Buffer.from('served-bytes')
  let status = 503
  t.mock.method(globalThis, 'fetch', async () => new Response(payload, { status }))
  await assert.rejects(download('https://example.com/a', sriOf(payload)), /download failed 503/)
  status = 200
  await assert.rejects(download('https://example.com/a', 'sha512-AAAA'), /integrity mismatch/)
  assert.deepEqual(await download('https://example.com/a', sriOf(payload)), payload)
})

test('extractArchive unpacks a tar.gz, removes the archive, throws on junk', t => {
  const dir = mkdtempSync(path.join(os.tmpdir(), 'soak-extract-'))
  t.after(() => rmSync(dir, { recursive: true, force: true }))
  writeFileSync(path.join(dir, 'hello.txt'), 'hi\n')
  spawnSync('tar', ['-czf', path.join(dir, 'a.tar.gz'), '-C', dir, 'hello.txt'])
  const buf = readFileSync(path.join(dir, 'a.tar.gz'))
  const dest = mkdtempSync(path.join(os.tmpdir(), 'soak-extract-dest-'))
  t.after(() => rmSync(dest, { recursive: true, force: true }))
  extractArchive('t', dest, 'a.tar.gz', buf)
  assert.ok(existsSync(path.join(dest, 'hello.txt')))
  assert.ok(!existsSync(path.join(dest, 'a.tar.gz')))
  assert.throws(() => extractArchive('t', dest, 'bad.tgz', Buffer.from('junk')), /extract failed/)
})

test('installTool rejects unknown tools, foreign purls, and shapeless pins', async () => {
  await assert.rejects(installTool('ghost', {}), /unknown tool/)
  await assert.rejects(
    installTool('x', { x: { purl: 'pkg:pypi/foo@1.0.0', integrity: GOOD_SRI } }),
    /unsupported purl/,
  )
  await assert.rejects(installTool('y', { y: { version: '1.0.0' } }), /no installable shape/)
  await assert.rejects(
    installTool('z', { z: { release: 'asset', version: '1.0.0', platforms: {} } }),
    /no pinned asset for/,
  )
})

test('installTool resolves the sfw flavor from SOCKET_SECURITY_KEY', async t => {
  if (process.platform === 'win32') {
    t.skip('sfw shims are POSIX-only')
    return
  }
  // An empty tools record makes the resolved flavor observable in the
  // rejection message — no download is ever attempted.
  await withEnv('SOCKET_SECURITY_KEY', undefined, () =>
    assert.rejects(installTool('sfw', {}), /unknown tool sfw-free/),
  )
  await withEnv('SOCKET_SECURITY_KEY', 'sk_test', () =>
    assert.rejects(installTool('sfw', {}), /unknown tool sfw-enterprise/),
  )
})

test('installTool prints the pinned uvx line for uv-project pins without installing', async t => {
  const tools = JSON.parse(readFileSync(EXTERNAL_TOOLS_JSON, 'utf8')).tools
  const uv = Object.keys(tools).find(name => tools[name].release === 'uv-project')
  assert.ok(uv, 'the manifest is expected to pin at least one uv project')
  const lines: string[] = []
  t.mock.method(console, 'log', (...args: unknown[]) => {
    lines.push(args.join(' '))
  })
  await installTool(uv!, tools)
  const pin = tools[uv!]
  const repo = pin.repository.replace(/^github:/, '')
  assert.ok(
    lines.some(line =>
      line.includes(`uvx --from git+https://github.com/${repo}@${pin.version} ${uv}`),
    ),
    `missing pinned uvx command in: ${lines.join('\n')}`,
  )
})

// Glue: main's read-only CLI paths against the tracked manifest — the same
// gate CI runs, exercised in-process so main() itself stays covered.
test('main --print-bin and --check are read-only and exit 0', async () => {
  assert.equal(await main(['--print-bin']), 0)
  assert.equal(await main(['--check']), 0)
})

// End to end through the entrypoint guard: the CLI must resolve as main
// (realpath + file URL) and exit 0 on the tracked manifest.
test('CLI: node external-tools.mts --check exits 0', () => {
  const script = fileURLToPath(new URL('./external-tools.mts', import.meta.url))
  const res = spawnSync(process.execPath, [script, '--check'], { encoding: 'utf8' })
  assert.equal(res.status, 0, res.stderr)
})
