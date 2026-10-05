import * as core from "@actions/core";
import * as semver from "semver";
import { getLatestKnownVersion } from "./download/checksum/known-version";
import {
  type ChecksumEntry,
  updateChecksums,
} from "./download/checksum/update-known-checksums";
import {
  fetchManifest,
  getLatestVersion,
  type ManifestVersion,
} from "./download/manifest";
import * as log from "./utils/logging";

async function run(): Promise<void> {
  const checksumFilePath = process.argv.slice(2)[0];
  if (!checksumFilePath) {
    throw new Error(
      "Missing checksum file path. Usage: node dist/update-known-checksums/index.cjs <checksum-file-path>",
    );
  }

  const latestVersion = await getLatestVersion();
  const latestKnownVersion = getLatestKnownVersion();

  if (semver.lte(latestVersion, latestKnownVersion)) {
    log.info(
      `Latest release (${latestVersion}) is not newer than the latest known version (${latestKnownVersion}). Skipping update.`,
    );
    return;
  }

  const versions = await fetchManifest();
  const checksumEntries = extractChecksumsFromManifest(versions);
  await updateChecksums(checksumFilePath, checksumEntries);

  core.setOutput("latest-version", latestVersion);
}

function extractChecksumsFromManifest(
  versions: ManifestVersion[],
): ChecksumEntry[] {
  const checksums: ChecksumEntry[] = [];

  for (const version of versions) {
    for (const artifact of version.artifacts) {
      checksums.push({
        checksum: artifact.sha256,
        key: `${artifact.platform}-${version.version}`,
      });
    }
  }

  return checksums;
}

run();
