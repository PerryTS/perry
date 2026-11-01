/**
 * Materialize the exact npm tarballs (ALL_PACKAGES) that CI will optionally scan and
 * then publish. The manifest is retained as workflow evidence and consumed by
 * the local GitHub-only release finalizer; it never contains credentials.
 */

import {
  existsSync,
  readdirSync,
  rmSync,
  statSync,
  writeFileSync,
} from 'node:fs'
import path from 'node:path'

import { ALL_PACKAGES, npmPackageDir, rootPath } from './constants.mts'
import {
  CI_PACKAGE_MANIFEST,
  type CiPackageManifest,
  type CiPackageProof,
} from './ci-package-manifest.mts'
import { readCargoVersion } from './npm/bump.mts'
import { readPackageJson } from './npm/shared.mts'
import { packTarball } from './npm/proof.mts'

async function main(): Promise<void> {
  const version = readCargoVersion()
  if (!version) throw new Error('could not read the workspace version from Cargo.toml')

  // Packed-size budget (bytes), set once in the workflow env. Unset = no check
  // (local runs). Every tarball is sized and printed first, so one failure
  // lists ALL the oversize packages, not just the first.
  const budgetText = process.env.NPM_PACKED_SIZE_BUDGET ?? ''
  const budget = budgetText === '' ? undefined : Number(budgetText)
  if (budget !== undefined && !(budget > 0)) {
    throw new Error(`NPM_PACKED_SIZE_BUDGET is not a positive number: ${budgetText}`)
  }
  const oversize: string[] = []

  const packages: CiPackageProof[] = []
  for (const name of ALL_PACKAGES) {
    const relativeDir = npmPackageDir(name)
    const packageDir = path.join(rootPath, relativeDir)
    const manifestPath = path.join(packageDir, 'package.json')
    if (!existsSync(manifestPath)) {
      throw new Error(`missing materialized package manifest: ${manifestPath}`)
    }
    const packageJson = readPackageJson(packageDir)
    if (packageJson.name !== name || packageJson.version !== version) {
      throw new Error(
        `${relativeDir}/package.json is ${String(packageJson.name)}@${String(packageJson.version)}; ` +
          `expected ${name}@${version}`,
      )
    }

    // A rerun in the same workspace must never select a stale archive.
    for (const file of readdirSync(packageDir)) {
      if (file.endsWith('.tgz')) rmSync(path.join(packageDir, file), { force: true })
    }
    const packed = await packTarball(packageDir)
    if (!packed) throw new Error(`npm pack failed for ${name}@${version}`)
    packages.push({
      name,
      version,
      path: path.relative(rootPath, packed.path),
      sha1: packed.sha1,
    })
    const size = statSync(packed.path).size
    console.log(
      `packed ${name}@${version}: ${packed.sha1}  ${path.relative(rootPath, packed.path)}  ${size} bytes (${(size / 1e6).toFixed(1)} MB)`,
    )
    if (budget !== undefined && size > budget) oversize.push(`${name}@${version} ${size}`)
  }
  if (oversize.length > 0) {
    throw new Error(
      `packed tarball(s) over the ${budget}-byte budget (npm rejects oversize uploads with 413): ${oversize.join(', ')}`,
    )
  }

  if (packages.length !== ALL_PACKAGES.length) {
    throw new Error(`expected ${ALL_PACKAGES.length} package proofs; got ${packages.length}`)
  }
  const manifest: CiPackageManifest = { packages, version }
  writeFileSync(
    path.join(rootPath, CI_PACKAGE_MANIFEST),
    `${JSON.stringify(manifest, null, 2)}\n`,
  )
}

await main()
