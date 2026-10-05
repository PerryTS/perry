import * as cache from "@actions/cache";
import * as core from "@actions/core";
import { hashFiles } from "../hash/hash-files";
import type { SetupInputs } from "../utils/inputs";
import * as log from "../utils/logging";
import { getArch, getOSNameVersion, getPlatform } from "../utils/platforms";

export const STATE_CACHE_KEY = "cache-key";
export const STATE_CACHE_MATCHED_KEY = "cache-matched-key";
export const STATE_PYTHON_CACHE_MATCHED_KEY = "python-cache-matched-key";

const CACHE_VERSION = "2";

export async function restoreCache(
  inputs: SetupInputs,
  pythonVersion?: string,
): Promise<void> {
  const cacheKey = await computeKeys(inputs, pythonVersion);
  core.saveState(STATE_CACHE_KEY, cacheKey);
  core.setOutput("cache-key", cacheKey);

  if (!inputs.restoreCache) {
    log.info("restore-cache is false. Skipping restore cache step.");
    core.setOutput("python-cache-hit", false);
    return;
  }

  if (inputs.cacheLocalPath === undefined) {
    throw new Error(
      "cache-local-path is not set. Cannot restore cache without a valid cache path.",
    );
  }

  await restoreCacheFromKey(
    cacheKey,
    inputs.cacheLocalPath.path,
    STATE_CACHE_MATCHED_KEY,
    "cache-hit",
  );

  if (inputs.cachePython) {
    await restoreCacheFromKey(
      `${cacheKey}-python`,
      inputs.pythonDir,
      STATE_PYTHON_CACHE_MATCHED_KEY,
      "python-cache-hit",
    );
  } else {
    core.setOutput("python-cache-hit", false);
  }
}

async function restoreCacheFromKey(
  cacheKey: string,
  cachePath: string,
  stateKey: string,
  outputKey: string,
): Promise<void> {
  log.info(
    `Trying to restore cache from GitHub Actions cache with key: ${cacheKey}`,
  );
  let matchedKey: string | undefined;
  try {
    matchedKey = await cache.restoreCache([cachePath], cacheKey);
  } catch (err) {
    const message = (err as Error).message;
    log.warning(message);
    core.setOutput(outputKey, false);
    return;
  }

  handleMatchResult(matchedKey, cacheKey, stateKey, outputKey);
}

async function computeKeys(
  inputs: SetupInputs,
  pythonVersion?: string,
): Promise<string> {
  let cacheDependencyPathHash = "-";
  if (inputs.cacheDependencyGlob !== "") {
    log.info(
      `Searching files using cache dependency glob: ${inputs.cacheDependencyGlob.split("\n").join(",")}`,
    );
    cacheDependencyPathHash += await hashFiles(
      inputs.cacheDependencyGlob,
      true,
    );
    if (cacheDependencyPathHash === "-") {
      log.warning(
        `No file matched to [${inputs.cacheDependencyGlob.split("\n").join(",")}]. The cache will never get invalidated. Make sure you have checked out the target repository and configured the cache-dependency-glob input correctly.`,
      );
    }
  }
  if (cacheDependencyPathHash === "-") {
    cacheDependencyPathHash = "-no-dependency-glob";
  }
  const suffix = inputs.cacheSuffix
    ? `-${encodeURIComponent(inputs.cacheSuffix)}`
    : "";
  const version = encodeURIComponent(pythonVersion ?? "unknown");
  const platform = await getPlatform();
  const osNameVersion = getOSNameVersion();
  const pruned = inputs.pruneCache ? "-pruned" : "";
  const python = inputs.cachePython ? "-py" : "";
  return `setup-uv-${CACHE_VERSION}-${getArch()}-${platform}-${osNameVersion}-${version}${pruned}${python}${cacheDependencyPathHash}${suffix}`;
}

function handleMatchResult(
  matchedKey: string | undefined,
  primaryKey: string,
  stateKey: string,
  outputKey: string,
): void {
  if (!matchedKey) {
    log.info(`No GitHub Actions cache found for key: ${primaryKey}`);
    core.setOutput(outputKey, false);
    return;
  }

  core.saveState(stateKey, matchedKey);
  log.info(`cache restored from GitHub Actions cache with key: ${matchedKey}`);
  core.setOutput(outputKey, true);
}
