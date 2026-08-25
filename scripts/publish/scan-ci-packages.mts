/** Socket-scan the exact tarballs prepared for direct OIDC publication. */

import { readFileSync, writeFileSync } from 'node:fs'
import path from 'node:path'

import { ALL_PACKAGES, rootPath } from './constants.mts'
import {
  CI_PACKAGE_MANIFEST,
  type CiPackageManifest,
} from './ci-package-manifest.mts'
import {
  preflightSocketScanAuth,
  scanTarball,
  type ScanResult,
} from './scan.mts'
export const SOCKET_SCAN_RECEIPT = 'socket-scan-receipt.json'

async function main(): Promise<void> {
  const manifest = JSON.parse(
    readFileSync(path.join(rootPath, CI_PACKAGE_MANIFEST), 'utf8'),
  ) as CiPackageManifest
  if (
    manifest.packages.length !== ALL_PACKAGES.length ||
    !ALL_PACKAGES.every(name => manifest.packages.some(pkg => pkg.name === name))
  ) {
    throw new Error('the CI package manifest does not contain Perry\'s exact nine-package set')
  }

  const ctx = await preflightSocketScanAuth()
  if (!ctx) process.exit(1)

  const results: ScanResult[] = []
  for (const pkg of manifest.packages) {
    results.push(
      await scanTarball(ctx, pkg.name, pkg.version, pkg.sha1, rootPath),
    )
  }
  const failed = results.filter(result => result.status !== 'passed')
  writeFileSync(
    path.join(rootPath, SOCKET_SCAN_RECEIPT),
    `${JSON.stringify({ status: failed.length === 0 ? 'passed' : 'failed', results }, null, 2)}\n`,
  )
  if (failed.length > 0) {
    throw new Error(
      `Socket rejected or could not scan: ${failed.map(result => `${result.name} (${result.status})`).join(', ')}`,
    )
  }
}

await main()
