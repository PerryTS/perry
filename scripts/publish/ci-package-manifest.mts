/** Shared shape for the exact npm tarballs packed, scanned, and published in CI. */

export const CI_PACKAGE_MANIFEST = 'npm-publish-manifest.json'

export interface CiPackageProof {
  name: string
  version: string
  path: string
  sha1: string
}

export interface CiPackageManifest {
  packages: CiPackageProof[]
  version: string
}
