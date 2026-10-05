import * as semver from "semver";
import { KNOWN_CHECKSUMS } from "./known-checksums";

const VERSION_IN_CHECKSUM_KEY_PATTERN =
  /-(\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?)$/;

export function getLatestKnownVersion(): string {
  const versions = new Set<string>();

  for (const key of Object.keys(KNOWN_CHECKSUMS)) {
    const version = key.match(VERSION_IN_CHECKSUM_KEY_PATTERN)?.[1];
    if (version !== undefined) {
      versions.add(version);
    }
  }

  const latestVersion = [...versions].sort(semver.rcompare)[0];
  if (!latestVersion) {
    throw new Error("Could not determine latest known version from checksums.");
  }

  return latestVersion;
}
