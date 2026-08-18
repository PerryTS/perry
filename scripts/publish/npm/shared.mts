/**
 * @file npm registry-read helpers: package.json reader, staged-entry shape +
 *   shasum reader, the `npm stage list` fetch, and prior-version lookup.
 *   Modeled on a tiered registry-infra design, pared to what
 *   Perry's 9-package staged flow needs.
 */

import { readFileSync } from 'node:fs'
import path from 'node:path'

import { extractFirstJson, rootPath, runCapture } from '../shared.mts'

/** A normalized staged entry from `npm stage list --json`. */
export interface StagedEntry {
  name: string
  version: string
  /** sha1 hex npm recorded for the staged tarball. */
  shasum?: string | undefined
  /** The staging id `npm stage approve <id>` promotes. */
  stageId: string
}

/** Read the subject package.json. */
export function readPackageJson(
  root: string = rootPath,
): { name?: string; version?: string } & Record<string, unknown> {
  try {
    return JSON.parse(readFileSync(path.join(root, 'package.json'), 'utf8'))
  } catch {
    return {}
  }
}

/**
 * Extract the staged tarball's sha1 from a `npm stage list --json` entry.
 * Live npm emits top-level `shasum`; `dist.shasum` stays as a fallback probe.
 * (`integrity` is sha512 — a different axis — not reduced to sha1 here.)
 */
export function readStagedShasum(entry: {
  dist?: { shasum?: unknown } | undefined
  shasum?: unknown
}): string | undefined {
  if (typeof entry.shasum === 'string' && entry.shasum) return entry.shasum
  if (typeof entry.dist?.shasum === 'string' && entry.dist.shasum) {
    return entry.dist.shasum
  }
  return undefined
}

/** A raw `npm stage list --json` entry across the shapes npm has emitted. */
interface RawStageEntry {
  id?: unknown
  stageId?: unknown
  name?: unknown
  packageName?: unknown
  version?: unknown
  shasum?: unknown
  dist?: { shasum?: unknown } | undefined
}

function normalizeEntry(raw: RawStageEntry): StagedEntry | undefined {
  const name =
    typeof raw.name === 'string'
      ? raw.name
      : typeof raw.packageName === 'string'
        ? raw.packageName
        : undefined
  const stageId =
    typeof raw.id === 'string' && raw.id
      ? raw.id
      : typeof raw.stageId === 'string' && raw.stageId
        ? raw.stageId
        : undefined
  if (!name || !stageId) return undefined
  const version = typeof raw.version === 'string' ? raw.version : ''
  return { name, version, stageId, shasum: readStagedShasum(raw) }
}

/**
 * Parse `npm stage list --json` output into normalized entries. Live npm
 * emits an array of `{ id, packageName, version, shasum, … }`; the older
 * keyed-map shape (`{ '<name>@<version>': { stageId, name, … } }`) is a
 * fallback.
 */
export function parseStageListJson(text: string): StagedEntry[] {
  const jsonText = extractFirstJson(text)
  if (!jsonText) return []
  let parsed: unknown
  try {
    parsed = JSON.parse(jsonText)
  } catch {
    return []
  }
  if (Array.isArray(parsed)) {
    return parsed
      .map((r: unknown) => normalizeEntry(r as RawStageEntry))
      .filter((e): e is StagedEntry => e !== undefined)
  }
  if (parsed && typeof parsed === 'object') {
    return Object.values(parsed)
      .map((r: unknown) => normalizeEntry(r as RawStageEntry))
      .filter((e): e is StagedEntry => e !== undefined)
  }
  return []
}

/** Run `npm stage list --json` and return normalized staged entries. */
export async function listStagedEntries(cwd: string): Promise<StagedEntry[]> {
  const { stdout, code } = await runCapture('npm', ['stage', 'list', '--json'], cwd)
  if (code !== 0) return []
  return parseStageListJson(stdout)
}

/**
 * True when `<name>@<version>` is already staged (matching stageId optional).
 * Used to keep the stage-upload idempotent — a re-stage skips entries already
 * in staging.
 */
export function isStagingExpected(
  entries: readonly StagedEntry[],
  name: string,
  version: string,
): StagedEntry | undefined {
  return entries.find(
    e => e.name === name && e.version === version,
  )
}

/** True when `<name>@<version>` is resolvable on the public registry. */
export async function fetchPublishedVersion(
  name: string,
  version: string,
): Promise<boolean> {
  const { code } = await runCapture('npm', ['view', `${name}@${version}`, 'version'], rootPath)
  return code === 0
}
