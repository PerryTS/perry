/**
 * @file npm manifest and anonymous public-registry read helpers.
 */

import { readFileSync } from 'node:fs'
import path from 'node:path'

import { rootPath, runCapture } from '../shared.mts'

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

/** True when `<name>@<version>` is resolvable on the public registry. */
export async function fetchPublishedVersion(
  name: string,
  version: string,
): Promise<boolean> {
  const { code } = await runCapture('npm', ['view', `${name}@${version}`, 'version'], rootPath)
  return code === 0
}

/** Normalize `npm view ... dist.shasum` without accepting noisy output. */
export function normalizePublishedShasum(
  stdout: string,
  code: number,
): string | undefined {
  const shasum = stdout.trim()
  return code === 0 && /^[0-9a-f]{40}$/i.test(shasum)
    ? shasum.toLowerCase()
    : undefined
}

/** Read the immutable registry sha1 for an already-public exact version. */
export async function fetchPublishedShasum(
  name: string,
  version: string,
): Promise<string | undefined> {
  const { code, stdout } = await runCapture(
    'npm',
    ['view', `${name}@${version}`, 'dist.shasum'],
    rootPath,
  )
  return normalizePublishedShasum(stdout, code)
}
