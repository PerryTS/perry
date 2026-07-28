#!/usr/bin/env node
/**
 * @file Pinned external security tooling — download, verify, shim.
 *   `external-tools.json` (repo root) pins every tool to an exact version
 *   with a sha512 SRI integrity per platform asset. This script is the only
 *   way those tools reach a machine: nothing here trusts "latest".
 *
 *   - `--check`            validate every pin (shape, SRI prefix, soak
 *                          annotations on any soakBypass) — CI gate, no network
 *   - `--fix`              prune soakBypass annotations whose window has
 *                          cleared, then run the same checks (the scheduled
 *                          soak-autofix workflow commits the result so the
 *                          gate never sits red waiting for a human)
 *   - `--install <name>`   download + SRI-verify + install into the local
 *                          tool rack (see paths.mts RACK_DIR) with a PATH
 *                          handle in BIN_DIR
 *   - `--install-all`      every installable pin
 *   - `--shims`            write sfw shims (npm/yarn/pnpm/pip/pip3/uv/cargo)
 *                          into BIN_DIR so installs route through the firewall
 *   - `--print-bin`        print BIN_DIR (for `>> $GITHUB_PATH` in CI)
 *
 *   `sfw` resolves to sfw-enterprise when SOCKET_SECURITY_KEY is set (the
 *   one env var every Socket product reads), else sfw-free — free tier
 *   needs no key, so CI is firewalled from day one and upgrades itself
 *   when the repo secret lands.
 */

import { createHash } from 'node:crypto'
import { spawnSync } from 'node:child_process'
import {
  chmodSync,
  existsSync,
  mkdirSync,
  readFileSync,
  realpathSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from 'node:fs'
import path from 'node:path'
import process from 'node:process'
import { pathToFileURL } from 'node:url'

import { SOAK_DAYS, addDaysIso, isValidIsoDate, todayIso } from './constants.mts'
import {
  BIN_DIR,
  DOCKER_PREBAKE,
  EXTERNAL_TOOLS_JSON,
  PM_DEP_INSTALLERS,
  RACK_DIR,
  REPO_ROOT,
  SURFACES,
} from './paths.mts'

const SFW_ECOSYSTEMS = ['npm', 'yarn', 'pnpm', 'pip', 'pip3', 'uv', 'cargo']

interface PlatformPin {
  asset: string
  integrity: string
}

interface ToolPin {
  description?: string
  version?: string
  repository?: string
  release?: string
  binaryName?: string
  purl?: string
  integrity?: string
  platforms?: Record<string, PlatformPin>
  soakBypass?: { version: string; published: string; removable: string }
}

function loadTools(): Record<string, ToolPin> {
  return JSON.parse(readFileSync(EXTERNAL_TOOLS_JSON, 'utf8')).tools
}

function platformKey(): string {
  const osKey = { darwin: 'darwin', linux: 'linux', win32: 'win' }[process.platform]
  const archKey = { arm64: 'arm64', x64: 'x64' }[process.arch]
  if (!osKey || !archKey) {
    throw new Error(`unsupported platform ${process.platform}-${process.arch}`)
  }
  // musl vs glibc via the loader-presence heuristic. Without this a musl
  // host silently resolved the glibc pin (the `-musl` pnpm entries were
  // dead keys) and installed a binary that can't run; tools with no -musl
  // pin now fail loud with "no pinned asset" instead.
  const musl =
    process.platform === 'linux' &&
    (existsSync('/lib/ld-musl-x86_64.so.1') || existsSync('/lib/ld-musl-aarch64.so.1'))
  return `${osKey}-${archKey}${musl ? '-musl' : ''}`
}

function sriSha512(buf: Buffer): string {
  return `sha512-${createHash('sha512').update(buf).digest('base64')}`
}

export function checkPins(tools: Record<string, ToolPin>): string[] {
  const out: string[] = []
  for (const [name, pin] of Object.entries(tools)) {
    if (!pin.version && !pin.purl) {
      out.push(`${name}: no version or purl pin`)
    }
    const integrities = [
      ...(pin.integrity ? [pin.integrity] : []),
      ...Object.values(pin.platforms ?? {}).map(p => p.integrity),
    ]
    if (pin.release === 'asset' && integrities.length === 0) {
      out.push(`${name}: release asset without any integrity pin`)
    }
    for (const sri of integrities) {
      if (!/^sha512-[A-Za-z0-9+/]+={0,2}$/.test(sri)) {
        out.push(`${name}: integrity is not a sha512 SRI: ${sri}`)
      }
    }
    if (pin.soakBypass) {
      const { published, removable } = pin.soakBypass
      if (!isValidIsoDate(published) || !isValidIsoDate(removable)) {
        out.push(`${name}: soakBypass dates are not real YYYY-MM-DD calendar dates`)
        continue
      }
      const expected = addDaysIso(published, SOAK_DAYS)
      if (removable !== expected) {
        out.push(`${name}: soakBypass removable ${removable}, wanted ${expected} (published + ${SOAK_DAYS}d)`)
      }
    }
  }
  return out
}

/**
 * Expired soakBypass annotations are STALE, not unsafe: the version has
 * soaked, the bypass no longer bypasses anything, and the pin stays
 * SRI-verified. They are reported as WARNINGS (exit 0), never failures —
 * a date boundary must not redden CI overnight with zero code change.
 * The soak-autofix workflow prunes them daily via --fix, so the ledger
 * still converges to clean. Missing/malformed/wrong-arithmetic
 * annotations stay hard checkPins failures: those are unauditable,
 * which IS unsafe.
 */
export function staleBypasses(tools: Record<string, ToolPin>): string[] {
  const out: string[] = []
  const today = todayIso()
  for (const [name, pin] of Object.entries(tools)) {
    const bypass = pin.soakBypass
    if (!bypass) {
      continue
    }
    if (!isValidIsoDate(bypass.published) || !isValidIsoDate(bypass.removable)) {
      continue
    }
    // Wrong-arithmetic annotations are a hard checkPins failure, never
    // stale/prunable — a too-early removable must not read as "cleared".
    if (bypass.removable !== addDaysIso(bypass.published, SOAK_DAYS)) {
      continue
    }
    if (bypass.removable < today) {
      out.push(name)
    }
  }
  return out
}

/**
 * Prune expired soakBypass annotations in place and return the pruned tool
 * names. Once `removable` is in the past the version has soaked and the
 * annotation is dead weight — staleBypasses warns about it until this
 * prunes it. Only valid, expired dates are pruned; malformed annotations
 * stay findings for a human (never silently rewritten).
 */
export function pruneExpiredSoakBypasses(doc: {
  tools: Record<string, ToolPin>
}): string[] {
  const pruned: string[] = []
  const today = todayIso()
  for (const [name, pin] of Object.entries(doc.tools)) {
    const bypass = pin.soakBypass
    if (!bypass) {
      continue
    }
    if (!isValidIsoDate(bypass.published) || !isValidIsoDate(bypass.removable)) {
      continue
    }
    // Wrong-arithmetic annotations are a hard checkPins failure, never
    // stale/prunable — a too-early removable must not read as "cleared".
    if (bypass.removable !== addDaysIso(bypass.published, SOAK_DAYS)) {
      continue
    }
    if (bypass.removable < today) {
      delete pin.soakBypass
      pruned.push(name)
    }
  }
  return pruned
}

function sriToHex(sri: string): string {
  return Buffer.from(sri.slice('sha512-'.length), 'base64').toString('hex')
}

/**
 * Parity gate for the CI agent image: its build context can't reach the
 * tracked pin sources, so the Dockerfile embeds copies of the sfw pin
 * (version + per-arch sha512 hex) and the toolchain channels. Assert the
 * copies match external-tools.json / rust-toolchain.toml so a pin bump
 * can't silently strand the image on old bits.
 */
export function checkDockerPrebake(
  dockerBody: string,
  tools: Record<string, ToolPin>,
  toolchainToml: string,
  rustVersion = '',
): string[] {
  const out: string[] = []
  const shimList = /for cmd in ([^;]+);/.exec(dockerBody)?.[1]?.trim().split(/\s+/)
  if (shimList && shimList.join(' ') !== SFW_ECOSYSTEMS.join(' ')) {
    out.push(`docker prebake: shim list [${shimList.join(' ')}] != SFW_ECOSYSTEMS [${SFW_ECOSYSTEMS.join(' ')}]`)
  }
  // Parse the install line's full argument list rather than substring-
  // matching: `rustup toolchain install 1.91.0 1.93.0` must satisfy an
  // msrv of 1.93 even though "toolchain install 1.93" never appears.
  const installedToolchains = [...dockerBody.matchAll(/toolchain install ([^\\\n]+)/g)]
    .flatMap(m => m[1]!.trim().split(/\s+/))
    .filter(a => /^\d/.test(a))
  if (
    rustVersion &&
    !installedToolchains.some(t => t === rustVersion || t.startsWith(`${rustVersion}.`))
  ) {
    out.push(`docker prebake: image does not pre-install the ${rustVersion} msrv toolchain`)
  }
  const sfw = tools['sfw-free']
  const version = /rack\/sfw-free\/([^/\s]+)\//.exec(dockerBody)?.[1]
  if (version !== sfw?.version) {
    out.push(
      `docker prebake: sfw-free version ${version ?? '(missing)'} != external-tools.json ${sfw?.version}`,
    )
  }
  const urlVersion = /sfw-free\/releases\/download\/v([^/\s]+)\//.exec(dockerBody)?.[1]
  if (urlVersion !== sfw?.version) {
    out.push(
      `docker prebake: sfw-free download url v${urlVersion ?? '(missing)'} != external-tools.json ${sfw?.version}`,
    )
  }
  const pairs = [...dockerBody.matchAll(/asset=(\S+);\s*sha=([0-9a-f]{128})/g)]
  if (pairs.length === 0) {
    out.push('docker prebake: no asset/sha pin pairs found in the Dockerfile')
  }
  for (const [, asset, hex] of pairs) {
    const plat = Object.values(sfw?.platforms ?? {}).find(p => p.asset === asset)
    if (!plat) {
      out.push(`docker prebake: asset ${asset} has no pin in external-tools.json`)
      continue
    }
    if (sriToHex(plat.integrity) !== hex) {
      out.push(`docker prebake: sha for ${asset} != hex of external-tools.json SRI`)
    }
  }
  const channel = /^channel\s*=\s*"([^"]+)"/m.exec(toolchainToml)?.[1]
  if (channel && !dockerBody.includes(`toolchain install ${channel} `)) {
    out.push(`docker prebake: image does not pre-install the pinned toolchain ${channel}`)
  }
  return out
}

export async function download(url: string, expectedSri: string): Promise<Buffer> {
  // Only GitHub gets the token (private release assets); sending it to any
  // other host (e.g. the npm registry for purl tools) would leak the
  // credential. Cross-origin redirects strip the header automatically.
  const token =
    process.env.GITHUB_TOKEN && new URL(url).hostname === 'github.com'
      ? process.env.GITHUB_TOKEN
      : ''
  // Fail fast on a stalled release/registry response instead of hanging
  // CI; 120s is generous for the largest pinned binary on a slow runner.
  const attempt = (withAuth: boolean) =>
    fetch(url, {
      headers: withAuth && token ? { authorization: `Bearer ${token}` } : {},
      redirect: 'follow',
      signal: AbortSignal.timeout(120_000),
    })
  let res = await attempt(Boolean(token))
  // A token that a PUBLIC cross-repo asset endpoint rejects (or a
  // transient GitHub 5xx — observed: a 500 on the first authed fetch of a
  // public sfw asset) must not fail the install outright: retry once
  // WITHOUT auth before giving up. Public assets need no credential.
  if (!res.ok && token) {
    res = await attempt(false)
  }
  if (!res.ok && res.status >= 500) {
    await new Promise(r => setTimeout(r, 2_000))
    res = await attempt(false)
  }
  if (!res.ok) {
    throw new Error(`download failed ${res.status} ${url}`)
  }
  const buf = Buffer.from(await res.arrayBuffer())
  const actual = sriSha512(buf)
  if (actual !== expectedSri) {
    throw new Error(`integrity mismatch for ${url}\n  expected ${expectedSri}\n  actual   ${actual}`)
  }
  return buf
}

export function extractArchive(name: string, destDir: string, asset: string, buf: Buffer): void {
  const archive = path.join(destDir, asset)
  writeFileSync(archive, buf)
  // bsdtar extracts zip via plain -xf too (macOS runners ship it as `tar`).
  // Windows needs BOTH quirks handled: Git Bash's PATH shadows System32's
  // bsdtar with GNU tar (which can't read zip — "does not look like a tar
  // archive"), so address the System32 binary explicitly; and tar parses an
  // absolute `C:\...` archive path as a remote `host:path` ("Cannot connect
  // to C"), so the archive is addressed RELATIVE to a cwd.
  const tarBin =
    process.platform === 'win32'
      ? path.join(process.env.SystemRoot || 'C:\\Windows', 'System32', 'tar.exe')
      : 'tar'
  const flags = asset.endsWith('.zip') ? '-xf' : '-xzf'
  const res = spawnSync(tarBin, [flags, asset], { cwd: destDir, stdio: 'inherit' })
  rmSync(archive)
  if (res.status !== 0) {
    throw new Error(`${name}: archive extract failed`)
  }
}

export function linkHandle(target: string, name: string): void {
  mkdirSync(BIN_DIR, { recursive: true })
  const handle = path.join(BIN_DIR, name)
  // force also removes a DANGLING handle (existsSync would report false
  // for one and a bare symlink would then throw EEXIST).
  rmSync(handle, { force: true })
  if (process.platform === 'win32') {
    // A symlinked/copied handle breaks Windows SEA binaries: pnpm.exe
    // resolves its dist/ siblings from the handle's OWN directory, not the
    // rack. Forward to the absolute rack target instead — a .cmd for
    // cmd/pwsh and an extensionless bash shim for Git Bash. Non-.exe
    // targets are node entry scripts (registry-tarball tools). The handle
    // BASE must not keep a caller-supplied .exe suffix: pwsh resolves
    // `pnpm` to `pnpm.exe` first, and a bash text file wearing that name
    // is "not a valid application for this OS platform".
    const base = path.join(BIN_DIR, name.replace(/\.exe$/, ''))
    const viaExe = target.endsWith('.exe')
    rmSync(base, { force: true })
    rmSync(`${base}.cmd`, { force: true })
    rmSync(`${base}.exe`, { force: true })
    writeFileSync(
      `${base}.cmd`,
      viaExe ? `@echo off\r\n"${target}" %*\r\n` : `@echo off\r\nnode "${target}" %*\r\n`,
    )
    writeFileSync(
      base,
      viaExe
        ? `#!/usr/bin/env bash\nexec "${target}" "$@"\n`
        : `#!/usr/bin/env bash\nexec node "${target}" "$@"\n`,
    )
    chmodSync(base, 0o755)
    return
  }
  symlinkSync(target, handle)
}

async function installAssetTool(name: string, pin: ToolPin): Promise<void> {
  const plat = pin.platforms?.[platformKey()]
  if (!plat) {
    throw new Error(`${name}: no pinned asset for ${platformKey()}`)
  }
  // A platform pinned to a registry .tgz (pnpm has no darwin-x64 SEA
  // upstream) routes through the npm-tarball path instead.
  if (plat.asset.endsWith('.tgz')) {
    await installNpmTarball(name, name, pin.version!, plat.integrity)
    return
  }
  const repo = pin.repository!.replace(/^github:/, '')
  const url = `https://github.com/${repo}/releases/download/v${pin.version}/${plat.asset}`
  let binName = pin.binaryName ?? name
  const destDir = path.join(RACK_DIR, name, pin.version!)
  let destBin = path.join(destDir, binName)
  // Windows archives land as `<bin>.exe`; resolve the same suffix the
  // post-extract path does so a second --install is a no-op instead of a
  // forced re-download (which wedges on a restricted/offline runner).
  if (!existsSync(destBin) && existsSync(`${destBin}.exe`)) {
    destBin = `${destBin}.exe`
    binName = `${binName}.exe`
  }
  if (existsSync(destBin)) {
    linkHandle(destBin, binName)
    console.log(`[external-tools] ${name}@${pin.version} already installed`)
    return
  }
  console.log(`[external-tools] downloading ${name}@${pin.version} (${plat.asset})`)
  const buf = await download(url, plat.integrity)
  mkdirSync(destDir, { recursive: true })
  if (plat.asset.endsWith('.tar.gz') || plat.asset.endsWith('.zip')) {
    extractArchive(name, destDir, plat.asset, buf)
    // Windows archives ship `<bin>.exe`; resolve it before giving up, and
    // clean the partial dir so a retry re-extracts instead of wedging.
    if (!existsSync(destBin) && existsSync(`${destBin}.exe`)) {
      destBin = `${destBin}.exe`
      binName = `${binName}.exe`
    }
    if (!existsSync(destBin)) {
      rmSync(destDir, { recursive: true, force: true })
      throw new Error(`${name}: ${binName} not found in extracted archive`)
    }
  } else {
    writeFileSync(destBin, buf)
  }
  chmodSync(destBin, 0o755)
  linkHandle(destBin, binName)
  console.log(`[external-tools] installed ${name}@${pin.version} -> ${destBin}`)
}

/**
 * Registry-tarball install: verify against the pinned SRI, extract into the
 * rack, materialize runtime deps with npm (pnpm as fallback), and link a
 * node wrapper for the package's bin. Tarballs that bundle their
 * node_modules (npm does) skip the dependency install.
 */
async function installNpmTarball(
  name: string,
  pkg: string,
  version: string,
  integrity: string,
): Promise<void> {
  const base = pkg.split('/').pop()
  const url = `https://registry.npmjs.org/${pkg}/-/${base}-${version}.tgz`
  const destDir = path.join(RACK_DIR, name, version)
  const pkgDir = path.join(destDir, 'package')
  // Completion marker is the extracted manifest, not the directory: a
  // failed/interrupted extract leaves the dir behind and would otherwise
  // wedge every later install. Reset and redo instead.
  const manifest = path.join(pkgDir, 'package.json')
  if (!existsSync(manifest)) {
    rmSync(destDir, { recursive: true, force: true })
    console.log(`[external-tools] downloading ${name}@${version} (npm registry)`)
    const buf = await download(url, integrity)
    mkdirSync(destDir, { recursive: true })
    extractArchive(name, destDir, 'package.tgz', buf)
  }
  const pkgJson = JSON.parse(readFileSync(manifest, 'utf8'))
  const hasDeps = Object.keys(pkgJson.dependencies ?? {}).length > 0
  if (hasDeps && !existsSync(path.join(pkgDir, 'node_modules'))) {
    if (installDeps(name, pkgDir) !== 0) {
      rmSync(destDir, { recursive: true, force: true })
      throw new Error(`${name}: dependency install failed`)
    }
  }
  const bins =
    typeof pkgJson.bin === 'string' ? { [name]: pkgJson.bin } : (pkgJson.bin ?? {})
  const binRel = bins[name] ?? Object.values(bins)[0]
  if (binRel) {
    const binAbs = path.join(pkgDir, binRel as string)
    if (process.platform === 'win32') {
      // linkHandle writes node-invoking .cmd + bash forwarders for a
      // non-.exe target — no bash-only wrapper to strand under pwsh.
      linkHandle(binAbs, name)
    } else {
      const wrapper = path.join(RACK_DIR, name, `${name}-wrapper`)
      writeFileSync(wrapper, `#!/usr/bin/env bash\nexec node '${binAbs}' "$@"\n`)
      chmodSync(wrapper, 0o755)
      linkHandle(wrapper, name)
    }
  }
  console.log(`[external-tools] installed ${name}@${version}`)
}

function installDeps(name: string, pkgDir: string): number {
  for (const [cmd, ...args] of PM_DEP_INSTALLERS) {
    if (cmd!.includes('/') && !existsSync(cmd!)) {
      continue
    }
    console.log(`[external-tools] ${name}: installing deps via ${path.basename(cmd!)}`)
    const res = spawnSync(cmd!, args, { cwd: pkgDir, stdio: 'inherit' })
    if (res.error) {
      continue
    }
    return res.status ?? 1
  }
  console.error(`[external-tools] ${name}: no package manager available for deps`)
  return 1
}

export async function installTool(name: string, tools: Record<string, ToolPin>): Promise<void> {
  // `sfw` is a flavor pair: the enterprise binary when a Socket token is
  // present (repo secret), the keyless free tier otherwise. The firewall
  // shim mechanism is POSIX (bash shims, extension-less symlink handles) —
  // skip cleanly on Windows rather than install something unusable.
  if (name === 'sfw') {
    if (process.platform === 'win32') {
      console.log('[external-tools] sfw shims are POSIX-only — skipping on windows')
      return
    }
    name = process.env.SOCKET_SECURITY_KEY ? 'sfw-enterprise' : 'sfw-free'
  }
  const pin = tools[name]
  if (!pin) {
    throw new Error(`unknown tool ${name} (see external-tools.json)`)
  }
  if (pin.release === 'asset') {
    await installAssetTool(name, pin)
    return
  }
  if (pin.purl) {
    // npm-packaged scanner (agentshield): verify the registry tarball
    // against the pinned SRI, then run via the extracted package.
    const m = /^pkg:npm\/(.+)@([^@]+)$/.exec(pin.purl)
    if (!m) {
      throw new Error(`${name}: unsupported purl ${pin.purl}`)
    }
    await installNpmTarball(name, m[1]!, m[2]!, pin.integrity!)
    return
  }
  if (pin.repository?.startsWith('npm:')) {
    // Platform-agnostic registry tarball (npm itself ships this way): one
    // tarball, one integrity, pure JS run through node. Never installed
    // via `npm install -g npm` — no self-update path.
    await installNpmTarball(name, pin.repository.slice('npm:'.length), pin.version!, pin.integrity!)
    return
  }
  if (pin.release === 'uv-project') {
    // Git-SHA-pinned python project; not auto-installed (needs uv).
    const repo = pin.repository!.replace(/^github:/, '')
    console.log(
      `[external-tools] ${name} is a uv project — run: uvx --from git+https://github.com/${repo}@${pin.version} ${name}`,
    )
    return
  }
  throw new Error(`${name}: no installable shape (release=${pin.release ?? 'none'})`)
}

/**
 * The rack location of a pinned command's runnable entry, or '' when the
 * command isn't rack-pinned/installed. Resolved from the manifest + rack
 * contents (never from the bin handle: after a --shims run the handle is
 * the shim itself, and resolving through it wouldn't survive a re-run).
 */
export function rackRealFor(cmd: string, tools: Record<string, ToolPin>): string {
  const pin = tools[cmd]
  if (!pin) {
    return ''
  }
  const candidates = [
    path.join(RACK_DIR, cmd, `${cmd}-wrapper`),
    ...(pin.version ? [path.join(RACK_DIR, cmd, pin.version, pin.binaryName ?? cmd)] : []),
  ]
  return candidates.find(c => existsSync(c)) ?? ''
}

/**
 * sfw shims: tiny wrappers named after each package manager that route the
 * real invocation through the firewall. A sentinel env var breaks the
 * recursion when sfw itself re-invokes the tool; the real binary is found
 * by stripping the rack's bin dir out of PATH.
 */
export function writeShims(tools: Record<string, ToolPin>): void {
  if (process.platform === 'win32') {
    console.log('[external-tools] sfw shims are POSIX-only — skipping on windows')
    return
  }
  mkdirSync(BIN_DIR, { recursive: true })
  for (const cmd of SFW_ECOSYSTEMS) {
    const sentinel = `SFW_SHIM_ACTIVE_${cmd.replace(/[^A-Za-z0-9]/g, '_').toUpperCase()}`
    // When the command itself is rack-pinned (pnpm/npm), the shim wraps the
    // PINNED binary; unpinned commands resolve at run time by stripping the
    // shim dir out of PATH.
    const handle = path.join(BIN_DIR, cmd)
    const rackReal = rackRealFor(cmd, tools)
    const resolveReal = rackReal
      ? `REAL='${rackReal}'`
      : `CLEAN_PATH=$(printf '%s' "$PATH" | tr ':' '\\n' | grep -vFx '${BIN_DIR}' | paste -sd ':' -)
REAL=$(PATH="$CLEAN_PATH" command -v '${cmd}' || true)`
    const body = `#!/usr/bin/env bash
# sfw shim for ${cmd} — managed by scripts/soak/external-tools.mts --shims
set -euo pipefail
${resolveReal}
if [ -n "\${${sentinel}:-}" ] || [ -z "$REAL" ] || ! command -v sfw >/dev/null 2>&1; then
  # Fail-open must not be SILENT-open: say so once on stderr when the
  # firewall is missing (never on the sentinel re-entry path, where sfw
  # itself is the caller).
  if [ -z "\${${sentinel}:-}" ] && [ -n "$REAL" ]; then
    echo "[sfw-shim] sfw not on PATH — running ${cmd} unfirewalled" >&2
  fi
  [ -n "$REAL" ] && exec "$REAL" "$@"
  echo "${cmd}: not found" >&2; exit 127
fi
export ${sentinel}=1
# Enterprise sfw defaults to BLOCK for non-registry hosts
# (SFW_UNKNOWN_HOST_ACTION, parsed by the enterprise config), which
# breaks ordinary dev flows the day a Socket key lands. Only the
# enterprise build reads the var — it is inert for the free tier — so
# setting it unconditionally is safe.
export SFW_UNKNOWN_HOST_ACTION=ignore
exec sfw '${cmd}' "$@"
`
    // Remove the handle before writing: writeFileSync FOLLOWS a symlink, so
    // writing through a rack handle would overwrite the pinned binary itself
    // with the shim body (which then execs itself forever).
    rmSync(handle, { force: true })
    writeFileSync(handle, body)
    chmodSync(handle, 0o755)
  }
  console.log(`[external-tools] wrote sfw shims for ${SFW_ECOSYSTEMS.join(', ')} in ${BIN_DIR}`)
  console.log(`[external-tools] prepend ${BIN_DIR} to PATH to activate`)
}

export async function main(argv: string[] = process.argv.slice(2)): Promise<number> {
  if (argv.includes('--print-bin')) {
    console.log(BIN_DIR)
    return 0
  }
  if (argv.includes('--fix')) {
    const doc = JSON.parse(readFileSync(EXTERNAL_TOOLS_JSON, 'utf8'))
    const pruned = pruneExpiredSoakBypasses(doc)
    if (pruned.length > 0) {
      writeFileSync(EXTERNAL_TOOLS_JSON, `${JSON.stringify(doc, null, 2)}\n`)
      console.log(`[external-tools] pruned expired soakBypass: ${pruned.join(', ')}`)
    }
  }
  const tools = loadTools()
  if (argv.includes('--check') || argv.includes('--fix') || argv.length === 0) {
    const problems = checkPins(tools)
    if (DOCKER_PREBAKE) {
      const dockerAbs = path.join(REPO_ROOT, DOCKER_PREBAKE)
      const toolchainAbs = SURFACES.toolchainToml
        ? path.join(REPO_ROOT, SURFACES.toolchainToml)
        : ''
      if (existsSync(dockerAbs)) {
        problems.push(
          ...checkDockerPrebake(
            readFileSync(dockerAbs, 'utf8'),
            tools,
            existsSync(toolchainAbs) ? readFileSync(toolchainAbs, 'utf8') : '',
            /^rust-version\s*=\s*"([^"]+)"/m.exec(
              readFileSync(path.join(REPO_ROOT, 'Cargo.toml'), 'utf8'),
            )?.[1] ?? '',
          ),
        )
      }
    }
    for (const p of problems) {
      console.error(`[external-tools] ${p}`)
    }
    for (const name of staleBypasses(tools)) {
      console.warn(
        `[external-tools] warn: ${name} soakBypass has cleared — stale annotation, pruned by --fix / the soak-autofix workflow`,
      )
    }
    if (problems.length === 0) {
      console.log(`[external-tools] ${Object.keys(tools).length} pins valid`)
    }
    return problems.length === 0 ? 0 : 1
  }
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] === '--install') {
      await installTool(argv[++i]!, tools)
    }
  }
  if (argv.includes('--install-all')) {
    for (const name of Object.keys(tools)) {
      if (name === 'sfw-enterprise' || name === 'sfw-free') {
        continue
      }
      await installTool(name, tools)
    }
    await installTool('sfw', tools)
  }
  if (argv.includes('--shims')) {
    writeShims(tools)
  }
  return 0
}

// realpath + pathToFileURL so symlinked checkouts and paths needing URL
// encoding still register as the entrypoint (ESM realpaths import.meta.url).
const isMain =
  process.argv[1] && pathToFileURL(realpathSync(process.argv[1])).href === import.meta.url
if (isMain) {
  main().then(
    code => {
      process.exitCode = code
    },
    err => {
      console.error(`[external-tools] ${err.message}`)
      process.exitCode = 1
    },
  )
}
