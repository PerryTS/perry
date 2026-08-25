/**
 * @file Perry version source. Replaces a manifest-edit bump step (which edits a
 *   CHANGELOG.md section + version field) with Perry's STOP-conditions check:
 *   the workspace version in Cargo.toml MUST agree with CLAUDE.md's Current
 *   Version line, the vX.Y.Z tag MUST NOT already exist, and changelog.d/ MUST
 *   have fragments. These are the same gates release-packages.yml's preflight
 *   enforces — surfaced here so a local `publish:release` fails in seconds
 *   instead of after 40 min of CI builds.
 *
 *   Perry froze CHANGELOG.md at v0.5.1264; the bump never edits it. The bump
 *   commit (Cargo.toml + CLAUDE.md version line + changelog.d fragment) is
 *   landed on main by the existing flow before `publish:release` runs.
 */

import { existsSync, readdirSync, readFileSync } from 'node:fs'
import path from 'node:path'

import { CHANGELOG_D, rootPath } from '../constants.mts'
import { logger, runCapture } from '../shared.mts'

/** Read [workspace.package].version from Cargo.toml. */
export function readCargoVersion(cwd: string = rootPath): string | undefined {
  const cargo = path.join(cwd, 'Cargo.toml')
  if (!existsSync(cargo)) return undefined
  const text = readFileSync(cargo, 'utf8')
  let inSection = false
  for (const line of text.split(/\r?\n/)) {
    if (/^\[workspace\.package\]/.test(line)) {
      inSection = true
      continue
    }
    if (/^\[/.test(line)) inSection = false
    if (inSection && /^version\s*=/.test(line)) {
      const m = line.match(/"([^"]+)"/)
      return m ? m[1] : undefined
    }
  }
  return undefined
}

/** Read the `**Current Version:**` line from CLAUDE.md. */
export function readClaudeVersion(cwd: string = rootPath): string | undefined {
  const claude = path.join(cwd, 'CLAUDE.md')
  if (!existsSync(claude)) return undefined
  const text = readFileSync(claude, 'utf8')
  const m = text.match(/\*\*Current Version:\*\*\s*(\S+)/)
  return m ? m[1] : undefined
}

/** True when at least one `changelog.d/<number>-*.md` fragment exists. */
export function hasChangelogFragments(cwd: string = rootPath): boolean {
  const dir = path.join(cwd, CHANGELOG_D)
  if (!existsSync(dir)) return false
  return readdirSync(dir).some(f => /^\d+-.*\.md$/.test(f))
}

/** True when the vX.Y.Z tag exists locally or on origin. */
export async function tagExists(version: string, cwd: string = rootPath): Promise<boolean> {
  const tag = `refs/tags/v${version}`
  const local = await runCapture('git', ['rev-parse', '-q', '--verify', tag], cwd)
  if (local.code === 0) return true
  const remote = await runCapture('git', ['ls-remote', '--tags', 'origin', tag], cwd)
  if (remote.code !== 0) {
    throw new Error(
      `could not query origin for ${tag} (git ls-remote exited ${remote.code})`,
    )
  }
  return remote.stdout.includes(tag)
}

export interface VersionGate {
  version: string
  ok: boolean
  /** Non-empty when ok is false — the human-readable stop reason(s). */
  reasons: string[]
}

/**
 * The Perry version gate. Mirrors release-packages.yml preflight's STOP
 * conditions. Returns { ok, reasons } so the pipeline fails fast with a
 * precise pointer.
 */
export async function checkVersionGate(cwd: string = rootPath): Promise<VersionGate> {
  const version = readCargoVersion(cwd)
  const reasons: string[] = []
  if (!version) {
    reasons.push('could not parse [workspace.package].version from Cargo.toml')
    return { version: '', ok: false, reasons }
  }
  const claude = readClaudeVersion(cwd)
  if (claude !== version) {
    reasons.push(
      `Cargo.toml says ${version} but CLAUDE.md's Current Version line says ${claude ?? '(missing)'} — fix the drift before releasing.`,
    )
  }
  try {
    if (await tagExists(version, cwd)) {
      reasons.push(
        `tag v${version} already exists — the version in Cargo.toml was already released. Land a version bump.`,
      )
    }
  } catch (err) {
    reasons.push(
      `${err instanceof Error ? err.message : String(err)} — refusing to assume the tag is absent.`,
    )
  }
  if (!hasChangelogFragments(cwd)) {
    reasons.push(
      'no fragments in changelog.d/ — the release notes are cut from them; nothing to release.',
    )
  }
  const ok = reasons.length === 0
  if (!ok) for (const r of reasons) logger.fail(r)
  return { version, ok, reasons }
}
