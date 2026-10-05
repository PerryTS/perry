import { homedir } from "node:os";
import { join } from "node:path";
import {
  mkdirSync,
  readdirSync,
  symlinkSync,
  renameSync,
  copyFileSync,
  existsSync,
} from "node:fs";
import { addPath, info, warning } from "@actions/core";
import { isFeatureAvailable, restoreCache } from "@actions/cache";
import { downloadTool, extractZip } from "@actions/tool-cache";
import { getExecOutput } from "@actions/exec";
import { Registry } from "./registry";
import { writeBunfig } from "./bunfig";
import { saveState } from "@actions/core";
import { addExtension, extractVersionFromUrl, getCacheKey } from "./utils";
import { getDownloadUrl } from "./download-url";
import { cwd } from "node:process";

export type Input = {
  customUrl?: string;
  version?: string;
  os?: string;
  arch?: string;
  avx2?: boolean;
  profile?: boolean;
  registries?: Registry[];
  noCache?: boolean;
  token?: string;
};

export type Output = {
  version: string;
  revision: string;
  bunPath: string;
  url: string;
  cacheHit: boolean;
};

export type CacheState = {
  cacheEnabled: boolean;
  cacheHit: boolean;
  bunPath: string;
  url: string;
};

export default async (options: Input): Promise<Output> => {
  const bunfigPath = join(cwd(), "bunfig.toml");
  writeBunfig(bunfigPath, options.registries);

  const url = await getDownloadUrl(options);
  const cacheEnabled = isCacheEnabled(options);

  const binPath = join(homedir(), ".bun", "bin");
  try {
    mkdirSync(binPath, { recursive: true });
  } catch (error) {
    if (error.code !== "EEXIST") {
      throw error;
    }
  }
  addPath(binPath);

  const exe = (name: string) =>
    process.platform === "win32" ? `${name}.exe` : name;
  const bunPath = join(binPath, exe("bun"));
  try {
    symlinkSync(bunPath, join(binPath, exe("bunx")));
  } catch (error) {
    if (error.code !== "EEXIST") {
      throw error;
    }
  }

  let revision: string | undefined;
  let cacheHit = false;

  // Check if Bun executable already exists and matches requested version
  if (!options.customUrl && existsSync(bunPath)) {
    const existingRevision = await getRevision(bunPath);
    if (existingRevision && isVersionMatch(existingRevision, options.version)) {
      revision = existingRevision;
      cacheHit = true; // Treat as cache hit to avoid unnecessary network requests
      info(`Using existing Bun installation: ${revision}`);
    }
  }

  if (!revision) {
    if (cacheEnabled) {
      const cacheKey = getCacheKey(url);

      const cacheRestored = await restoreCache([bunPath], cacheKey);
      if (cacheRestored) {
        revision = await getRevision(bunPath);
        if (revision) {
          const expectedVersion = extractVersionFromUrl(url);
          const [actualVersion] = revision.split("+");
          if (!expectedVersion) {
            warning(
              `Could not parse expected version from URL: ${url}. Ignoring cache.`,
            );
            revision = undefined;
          } else if (actualVersion !== expectedVersion) {
            warning(
              `Cached Bun version ${revision} does not match expected version ${expectedVersion}. Re-downloading.`,
            );
            revision = undefined;
          } else {
            cacheHit = true;
            info(`Using a cached version of Bun: ${revision}`);
          }
        } else {
          warning(
            `Found a cached version of Bun: ${revision} (but it appears to be corrupted?)`,
          );
        }
      }
    }

    if (!cacheHit) {
      info(`Downloading a new version of Bun: ${url}`);
      revision = await downloadBun(url, bunPath);
    }
  }

  if (!revision) {
    throw new Error(
      "Downloaded a new version of Bun, but failed to check its version? Try again.",
    );
  }

  const [version] = revision.split("+");

  const cacheState: CacheState = {
    cacheEnabled,
    cacheHit,
    bunPath,
    url,
  };

  saveState("cache", JSON.stringify(cacheState));

  return {
    version,
    revision,
    bunPath,
    url,
    cacheHit,
  };
};

function isVersionMatch(
  existingRevision: string,
  requestedVersion?: string,
): boolean {
  // If no version specified, default is "latest" - don't match existing
  if (!requestedVersion) {
    return false;
  }

  // Non-pinned versions should never match existing installations
  if (/^(latest|canary|action)$/i.test(requestedVersion)) {
    return false;
  }

  const [existingVersion] = existingRevision.split("+");

  const normalizeVersion = (v: string) => v.replace(/^v/i, "");

  return (
    normalizeVersion(existingVersion) === normalizeVersion(requestedVersion)
  );
}

async function downloadBun(
  url: string,
  bunPath: string,
): Promise<string | undefined> {
  // Workaround for https://github.com/oven-sh/setup-bun/issues/79 and https://github.com/actions/toolkit/issues/1179
  const zipPath = addExtension(await downloadTool(url), ".zip");
  const extractedZipPath = await extractZip(zipPath);
  const extractedBunPath = await extractBun(extractedZipPath);
  try {
    renameSync(extractedBunPath, bunPath);
  } catch {
    // If mv does not work, try to copy the file instead.
    // For example: EXDEV: cross-device link not permitted
    copyFileSync(extractedBunPath, bunPath);
  }

  return await getRevision(bunPath);
}

function isCacheEnabled(options: Input): boolean {
  const { customUrl, version, noCache } = options;
  if (noCache) {
    return false;
  }
  if (customUrl) {
    return false;
  }
  if (!version || /latest|canary|action/i.test(version)) {
    return false;
  }
  return isFeatureAvailable();
}

async function extractBun(path: string): Promise<string> {
  for (const entry of readdirSync(path, { withFileTypes: true })) {
    const { name } = entry;
    const entryPath = join(path, name);
    if (entry.isFile()) {
      if (name === "bun" || name === "bun.exe") {
        return entryPath;
      }
      if (/^bun.*\.zip/.test(name)) {
        const extractedPath = await extractZip(entryPath);
        return extractBun(extractedPath);
      }
    }
    if (/^bun/.test(name) && entry.isDirectory()) {
      return extractBun(entryPath);
    }
  }
  throw new Error("Could not find executable: bun");
}

async function getRevision(exe: string): Promise<string | undefined> {
  const revision = await getExecOutput(exe, ["--revision"], {
    ignoreReturnCode: true,
  });
  if (revision.exitCode === 0 && /^\d+\.\d+\.\d+/.test(revision.stdout)) {
    return revision.stdout.trim();
  }
  const version = await getExecOutput(exe, ["--version"], {
    ignoreReturnCode: true,
  });
  if (version.exitCode === 0 && /^\d+\.\d+\.\d+/.test(version.stdout)) {
    return version.stdout.trim();
  }
  return undefined;
}
