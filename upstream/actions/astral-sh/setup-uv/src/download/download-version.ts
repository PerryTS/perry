import { promises as fs } from "node:fs";
import * as path from "node:path";
import * as core from "@actions/core";
import * as tc from "@actions/tool-cache";
import {
  ASTRAL_MIRROR_PREFIX,
  GITHUB_RELEASES_PREFIX,
  TOOL_CACHE_NAME,
  VERSIONS_MANIFEST_URL,
} from "../utils/constants";
import * as log from "../utils/logging";
import type { Architecture, Platform } from "../utils/platforms";
import { validateChecksum } from "./checksum/checksum";
import { getArtifact } from "./manifest";

export { resolveVersion } from "../version/resolve";

export function tryGetFromToolCache(
  arch: Architecture,
  version: string,
): { version: string; installedPath: string | undefined } {
  core.debug(`Trying to get uv from tool cache for ${version}...`);
  const cachedVersions = tc.findAllVersions(TOOL_CACHE_NAME, arch);
  core.debug(`Cached versions: ${cachedVersions}`);
  let resolvedVersion = tc.evaluateVersions(cachedVersions, version);
  if (resolvedVersion === "") {
    resolvedVersion = version;
  }
  const installedPath = tc.find(TOOL_CACHE_NAME, resolvedVersion, arch);
  return { installedPath, version: resolvedVersion };
}

export async function downloadVersion(
  platform: Platform,
  arch: Architecture,
  version: string,
  checksum: string | undefined,
  githubToken: string,
  manifestUrl?: string,
  downloadFromAstralMirror = true,
): Promise<{ version: string; cachedToolDir: string }> {
  const artifact = await getArtifact(version, arch, platform, manifestUrl);

  if (!artifact) {
    throw new Error(
      getMissingArtifactMessage(version, arch, platform, manifestUrl),
    );
  }

  // Custom manifests are explicitly selected by the user, so their checksum
  // takes precedence over the built-in table. For the default manifest, pass
  // its checksum as a fallback after user input and KNOWN_CHECKSUMS.
  const resolvedChecksum =
    manifestUrl === undefined
      ? checksum
      : resolveChecksum(checksum, artifact.checksum);
  const manifestChecksum = artifact.checksum;

  const mirrorUrl = downloadFromAstralMirror
    ? rewriteToMirror(artifact.downloadUrl)
    : undefined;
  const downloadUrl = mirrorUrl ?? artifact.downloadUrl;

  try {
    return await downloadArtifact(
      downloadUrl,
      `uv-${arch}-${platform}`,
      platform,
      arch,
      version,
      resolvedChecksum,
      manifestChecksum,
      githubTokenForUrl(downloadUrl, githubToken),
    );
  } catch (err) {
    if (mirrorUrl === undefined) {
      throw err;
    }

    log.warning(
      `Failed to download from mirror, falling back to GitHub Releases: ${(err as Error).message}`,
    );

    return await downloadArtifact(
      artifact.downloadUrl,
      `uv-${arch}-${platform}`,
      platform,
      arch,
      version,
      resolvedChecksum,
      manifestChecksum,
      githubTokenForUrl(artifact.downloadUrl, githubToken),
    );
  }
}

/**
 * Rewrite a GitHub Releases URL to the Astral mirror.
 * Returns `undefined` if the URL does not match the expected GitHub prefix.
 */
export function rewriteToMirror(url: string): string | undefined {
  if (!url.startsWith(GITHUB_RELEASES_PREFIX)) {
    return undefined;
  }

  return ASTRAL_MIRROR_PREFIX + url.slice(GITHUB_RELEASES_PREFIX.length);
}

function githubTokenForUrl(
  downloadUrl: string,
  githubToken: string,
): string | undefined {
  try {
    return new URL(downloadUrl).origin === "https://github.com"
      ? githubToken
      : undefined;
  } catch {
    return undefined;
  }
}

async function downloadArtifact(
  downloadUrl: string,
  artifactName: string,
  platform: Platform,
  arch: Architecture,
  version: string,
  checksum: string | undefined,
  manifestChecksum: string | undefined,
  githubToken: string | undefined,
): Promise<{ version: string; cachedToolDir: string }> {
  log.info(`Downloading uv from "${downloadUrl}" ...`);
  const downloadPath = await tc.downloadTool(
    downloadUrl,
    undefined,
    githubToken,
  );
  await validateChecksum(
    checksum,
    downloadPath,
    arch,
    platform,
    version,
    manifestChecksum,
  );

  let uvDir: string;
  if (platform === "pc-windows-msvc") {
    // On windows extracting the zip does not create an intermediate directory.
    try {
      // Try tar first as it's much faster, but only bsdtar supports zip files,
      // so this may fail if another tar, like gnu tar, ends up being used.
      uvDir = await tc.extractTar(downloadPath, undefined, "x");
    } catch (err) {
      log.info(
        `Extracting with tar failed, falling back to zip extraction: ${(err as Error).message}`,
      );
      const extension = getExtension(platform);
      const fullPathWithExtension = `${downloadPath}${extension}`;
      await fs.copyFile(downloadPath, fullPathWithExtension);
      uvDir = await tc.extractZip(fullPathWithExtension);
    }
  } else {
    const extractedDir = await tc.extractTar(downloadPath);
    uvDir = path.join(extractedDir, artifactName);
  }

  const cachedToolDir = await tc.cacheDir(
    uvDir,
    TOOL_CACHE_NAME,
    version,
    arch,
  );
  return { cachedToolDir, version };
}

function getMissingArtifactMessage(
  version: string,
  arch: Architecture,
  platform: Platform,
  manifestUrl?: string,
): string {
  if (manifestUrl === undefined) {
    return `Could not find artifact for version ${version}, arch ${arch}, platform ${platform} in ${VERSIONS_MANIFEST_URL} .`;
  }

  return `manifest-file does not contain version ${version}, arch ${arch}, platform ${platform}.`;
}

function resolveChecksum(
  checksum: string | undefined,
  manifestChecksum: string,
): string {
  return checksum !== undefined && checksum !== ""
    ? checksum
    : manifestChecksum;
}

function getExtension(platform: Platform): string {
  return platform === "pc-windows-msvc" ? ".zip" : ".tar.gz";
}
