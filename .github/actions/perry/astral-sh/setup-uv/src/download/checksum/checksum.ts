import * as crypto from "node:crypto";
import * as fs from "node:fs";

import * as core from "@actions/core";
import type { Architecture, Platform } from "../../utils/platforms";
import { KNOWN_CHECKSUMS } from "./known-checksums";

export async function validateChecksum(
  checksum: string | undefined,
  downloadPath: string,
  arch: Architecture,
  platform: Platform,
  version: string,
  manifestChecksum?: string,
): Promise<void> {
  const key = `${arch}-${platform}-${version}`;
  const hasProvidedChecksum = checksum !== undefined && checksum !== "";
  const knownChecksum = KNOWN_CHECKSUMS[key];
  const hasManifestChecksum =
    manifestChecksum !== undefined && manifestChecksum !== "";
  const checksumToUse = hasProvidedChecksum
    ? checksum
    : (knownChecksum ?? (hasManifestChecksum ? manifestChecksum : undefined));

  if (checksumToUse === undefined) {
    if (manifestChecksum !== undefined) {
      throw new Error(`No checksum found for ${key} in manifest.`);
    }
    core.debug(`No checksum found for ${key}.`);
    return;
  }

  const checksumSource = hasProvidedChecksum
    ? "provided checksum"
    : knownChecksum !== undefined
      ? `KNOWN_CHECKSUMS entry for ${key}`
      : "manifest checksum";

  core.debug(`Validating checksum using ${checksumSource}.`);
  const isValid = await validateFileCheckSum(downloadPath, checksumToUse);

  if (!isValid) {
    throw new Error(
      `Checksum for ${downloadPath} did not match ${checksumToUse}.`,
    );
  }

  core.debug(`Checksum for ${downloadPath} is valid.`);
}

async function validateFileCheckSum(
  filePath: string,
  expected: string,
): Promise<boolean> {
  return new Promise((resolve, reject) => {
    const hash = crypto.createHash("sha256");
    const stream = fs.createReadStream(filePath);
    stream.on("error", (err) => reject(err));
    stream.on("data", (chunk) => hash.update(chunk));
    stream.on("end", () => {
      const actual = hash.digest("hex");
      resolve(actual === expected);
    });
  });
}

export function isknownVersion(version: string): boolean {
  const pattern = new RegExp(`^.*-.*-${version}$`);
  return Object.keys(KNOWN_CHECKSUMS).some((key) => pattern.test(key));
}
