/** Pack and resolve the exact npm tarballs used for scan/publish proof. */

import { createHash } from 'node:crypto'
import { createReadStream, existsSync, readdirSync } from 'node:fs'
import path from 'node:path'

import { npmPackageDir, rootPath } from '../constants.mts'
import { runCapture } from '../shared.mts'

/** Pack one package directory without lifecycle scripts. */
export async function packTarball(pkgDir: string): Promise<{
  path: string
  sha1: string
} | undefined> {
  const { stdout, code } = await runCapture(
    'npm',
    ['pack', '--json', '--ignore-scripts'],
    pkgDir,
  )
  // stderr is inherited (see runCapture), so npm's own errors are already in
  // the log. What was NOT visible is the payload we failed to parse — that gap
  // cost a full release cycle in run 34335433079, where the only symptom was
  // "npm pack failed" even though pack exited 0 and wrote the tarball.
  if (code !== 0) {
    console.error(`npm pack exited ${code} in ${pkgDir}`)
    return undefined
  }
  let parsed: unknown
  try {
    parsed = JSON.parse(stdout)
  } catch {
    console.error(`npm pack --json emitted unparseable output in ${pkgDir}:`)
    console.error(stdout.slice(0, 2000))
    return undefined
  }
  // The shape changed at npm 12: v11 emits an ARRAY of entries, v12 an OBJECT
  // keyed by package name. Accept both.
  //   v11: [ { filename, shasum, ... } ]
  //   v12: { "@scope/name": { filename, shasum, ... } }
  const entry = Array.isArray(parsed)
    ? parsed[0]
    : parsed !== null && typeof parsed === 'object'
      ? Object.values(parsed as Record<string, unknown>)[0]
      : undefined
  if (!entry || typeof entry !== 'object') {
    console.error(`npm pack --json gave no usable entry in ${pkgDir}:`)
    console.error(stdout.slice(0, 2000))
    return undefined
  }
  const filename = (entry as { filename?: unknown }).filename
  const shasum = (entry as { shasum?: unknown }).shasum
  if (typeof filename !== 'string' || typeof shasum !== 'string') return undefined
  const tarball = path.join(pkgDir, filename)
  if (!existsSync(tarball)) return undefined
  return { path: tarball, sha1: shasum }
}

/** Pack locally, or resolve the one retained CI proof tarball for a package. */
export async function resolvePackageTarball(
  name: string,
  proofRoot?: string,
): Promise<{ path: string; sha1: string } | undefined> {
  const pkgDir = path.join(proofRoot ?? rootPath, npmPackageDir(name))
  if (!proofRoot) return packTarball(pkgDir)
  if (!existsSync(pkgDir)) return undefined
  const tarballs = readdirSync(pkgDir).filter(file => file.endsWith('.tgz'))
  if (tarballs.length !== 1) return undefined
  const tarball = path.join(pkgDir, tarballs[0]!)
  const sha1 = await new Promise<string | undefined>(resolve => {
    const hash = createHash('sha1')
    createReadStream(tarball)
      .on('data', chunk => hash.update(chunk))
      .on('error', () => resolve(undefined))
      .on('end', () => resolve(hash.digest('hex')))
  })
  return sha1 ? { path: tarball, sha1 } : undefined
}

/** Verify a retained CI tarball against its recorded sha1. */
export async function verifyPackageProof(
  name: string,
  expectedSha1: string,
  proofRoot: string,
): Promise<boolean> {
  const proof = await resolvePackageTarball(name, proofRoot)
  return proof?.sha1 === expectedSha1
}
