import { createInterface } from "node:readline";
import { Readable } from "node:stream";
import * as core from "@actions/core";
import { VERSIONS_MANIFEST_URL } from "../utils/constants";
import { fetch } from "../utils/fetch";
import * as log from "../utils/logging";
import { selectDefaultVariant } from "./variant-selection";

export interface ManifestArtifact {
  platform: string;
  variant?: string;
  url: string;
  archive_format: string;
  sha256: string;
}

export interface ManifestVersion {
  version: string;
  artifacts: ManifestArtifact[];
}

export interface ArtifactResult {
  archiveFormat: string;
  checksum: string;
  downloadUrl: string;
}

interface CachedManifest {
  complete: boolean;
  versions: ManifestVersion[];
}

const cachedManifestData = new Map<string, CachedManifest>();
export const MANIFEST_FETCH_ATTEMPTS = 3;

export async function fetchManifest(
  manifestUrl: string = VERSIONS_MANIFEST_URL,
): Promise<ManifestVersion[]> {
  const cachedManifest = cachedManifestData.get(manifestUrl);
  if (cachedManifest?.complete === true) {
    core.debug(`Using cached manifest data from ${manifestUrl}`);
    return cachedManifest.versions;
  }

  const response = await fetchManifestResponse(manifestUrl);
  const body = await response.text();
  const versions = parseManifest(body, manifestUrl);
  cachedManifestData.set(manifestUrl, { complete: true, versions });
  return versions;
}

export function parseManifest(
  data: string,
  sourceDescription: string,
): ManifestVersion[] {
  const trimmed = data.trim();
  if (trimmed === "") {
    throw new Error(`Manifest at ${sourceDescription} is empty.`);
  }

  rejectLegacyManifest(trimmed, sourceDescription);

  const versions: ManifestVersion[] = [];

  for (const [index, line] of data.split("\n").entries()) {
    const record = line.trim();
    if (record === "") {
      continue;
    }

    versions.push(parseManifestRecord(record, sourceDescription, index + 1));
  }

  if (versions.length === 0) {
    throw new Error(`No manifest data found in ${sourceDescription}.`);
  }

  return versions;
}

export async function getLatestVersion(
  manifestUrl: string = VERSIONS_MANIFEST_URL,
): Promise<string> {
  const latestVersion =
    manifestUrl === VERSIONS_MANIFEST_URL
      ? (await findManifestVersion(() => true))?.version
      : (await fetchManifest(manifestUrl))[0]?.version;

  if (latestVersion === undefined) {
    throw new Error("No versions found in manifest data");
  }

  core.debug(`Latest version from manifest: ${latestVersion}`);
  return latestVersion;
}

// The default manifest is guaranteed to be ordered newest-first:
// https://github.com/astral-sh/versions#format
export async function getFirstMatchingVersion(
  predicate: (version: string) => boolean,
): Promise<string | undefined> {
  return (
    await findManifestVersion((versionData) => predicate(versionData.version))
  )?.version;
}

export async function getAllVersions(
  manifestUrl: string = VERSIONS_MANIFEST_URL,
): Promise<string[]> {
  log.info(
    `Getting available versions from ${manifestSource(manifestUrl)} ...`,
  );
  const versions = await fetchManifest(manifestUrl);
  return versions.map((versionData) => versionData.version);
}

export async function getArtifact(
  version: string,
  arch: string,
  platform: string,
  manifestUrl: string = VERSIONS_MANIFEST_URL,
): Promise<ArtifactResult | undefined> {
  const versionData =
    manifestUrl === VERSIONS_MANIFEST_URL
      ? await findManifestVersion((candidate) => candidate.version === version)
      : (await fetchManifest(manifestUrl)).find(
          (candidate) => candidate.version === version,
        );
  if (!versionData) {
    core.debug(`Version ${version} not found in manifest ${manifestUrl}`);
    return undefined;
  }

  const targetPlatform = `${arch}-${platform}`;
  const matchingArtifacts = versionData.artifacts.filter(
    (candidate) => candidate.platform === targetPlatform,
  );

  if (matchingArtifacts.length === 0) {
    core.debug(
      `Artifact for ${targetPlatform} not found in version ${version}. Available platforms: ${versionData.artifacts
        .map((candidate) => candidate.platform)
        .join(", ")}`,
    );
    return undefined;
  }

  const artifact = selectDefaultVariant(
    matchingArtifacts,
    `Multiple artifacts found for ${targetPlatform} in version ${version}`,
  );

  return {
    archiveFormat: artifact.archive_format,
    checksum: artifact.sha256,
    downloadUrl: artifact.url,
  };
}

export function clearManifestCache(manifestUrl?: string): void {
  if (manifestUrl === undefined) {
    cachedManifestData.clear();
    return;
  }

  cachedManifestData.delete(manifestUrl);
}

async function fetchManifestResponse(manifestUrl: string) {
  let response: Awaited<ReturnType<typeof fetch>> | undefined;
  for (let attempt = 1; attempt <= MANIFEST_FETCH_ATTEMPTS; attempt++) {
    log.info(`Fetching manifest data from ${manifestUrl} ...`);
    try {
      response = await fetch(manifestUrl, {});
      break;
    } catch (error) {
      if (attempt >= MANIFEST_FETCH_ATTEMPTS) {
        throw error;
      }
      const delayMs = 1_000 * 2 ** (attempt - 1);
      log.info(`Manifest fetch failed; retrying in ${delayMs}ms ...`);
      await new Promise((resolve) => setTimeout(resolve, delayMs));
    }
  }

  if (response === undefined) {
    throw new Error("Manifest fetch attempts exhausted.");
  }

  if (!response.ok) {
    throw new Error(
      `Failed to fetch manifest data: ${response.status} ${response.statusText}`,
    );
  }

  return response;
}

async function findManifestVersion(
  predicate: (versionData: ManifestVersion) => boolean,
): Promise<ManifestVersion | undefined> {
  const cachedManifest = cachedManifestData.get(VERSIONS_MANIFEST_URL);
  const cachedVersion = cachedManifest?.versions.find(predicate);
  if (cachedVersion !== undefined || cachedManifest?.complete === true) {
    return cachedVersion;
  }

  const response = await fetchManifestResponse(VERSIONS_MANIFEST_URL);
  if (response.body === null) {
    const versions = parseManifest(
      await response.text(),
      VERSIONS_MANIFEST_URL,
    );
    cachedManifestData.set(VERSIONS_MANIFEST_URL, {
      complete: true,
      versions,
    });
    return versions.find(predicate);
  }

  const input = Readable.fromWeb(response.body);
  const lines = createInterface({ crlfDelay: Number.POSITIVE_INFINITY, input });
  const versions: ManifestVersion[] = [];
  let complete = false;
  let lineNumber = 0;
  let matchedVersion: ManifestVersion | undefined;

  try {
    for await (const line of lines) {
      lineNumber += 1;
      const record = line.trim();
      if (record === "") {
        continue;
      }

      if (versions.length === 0) {
        rejectLegacyManifest(record, VERSIONS_MANIFEST_URL);
      }

      const versionData = parseManifestRecord(
        record,
        VERSIONS_MANIFEST_URL,
        lineNumber,
      );
      versions.push(versionData);
      if (predicate(versionData)) {
        matchedVersion = versionData;
        break;
      }
    }

    complete = matchedVersion === undefined;
  } finally {
    lines.close();
    if (!complete) {
      input.destroy();
    }
  }

  if (versions.length === 0) {
    throw new Error(`Manifest at ${VERSIONS_MANIFEST_URL} is empty.`);
  }

  cachedManifestData.set(VERSIONS_MANIFEST_URL, { complete, versions });
  return matchedVersion;
}

function manifestSource(manifestUrl: string): string {
  if (manifestUrl === VERSIONS_MANIFEST_URL) {
    return VERSIONS_MANIFEST_URL;
  }

  return `manifest-file ${manifestUrl}`;
}

function parseManifestRecord(
  record: string,
  sourceDescription: string,
  lineNumber: number,
): ManifestVersion {
  let parsed: unknown;
  try {
    parsed = JSON.parse(record);
  } catch (error) {
    throw new Error(
      `Failed to parse manifest data from ${sourceDescription} at line ${lineNumber}: ${(error as Error).message}`,
    );
  }

  if (!isManifestVersion(parsed)) {
    throw new Error(
      `Invalid manifest record in ${sourceDescription} at line ${lineNumber}.`,
    );
  }

  return parsed;
}

function rejectLegacyManifest(data: string, sourceDescription: string): void {
  if (data.startsWith("[")) {
    throw new Error(
      `Legacy JSON array manifests are no longer supported in ${sourceDescription}. Use the astral-sh/versions manifest format instead.`,
    );
  }
}

function isManifestVersion(value: unknown): value is ManifestVersion {
  if (!isRecord(value)) {
    return false;
  }

  if (typeof value.version !== "string" || !Array.isArray(value.artifacts)) {
    return false;
  }

  return value.artifacts.every(isManifestArtifact);
}

function isManifestArtifact(value: unknown): value is ManifestArtifact {
  if (!isRecord(value)) {
    return false;
  }

  const variantIsValid =
    typeof value.variant === "string" || value.variant === undefined;

  return (
    typeof value.archive_format === "string" &&
    typeof value.platform === "string" &&
    typeof value.sha256 === "string" &&
    typeof value.url === "string" &&
    variantIsValid
  );
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}
