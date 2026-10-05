import * as fs from "node:fs";
import * as cache from "@actions/cache";
import * as core from "@actions/core";
import * as exec from "@actions/exec";
import * as pep440 from "@renovatebot/pep440";
import {
  STATE_CACHE_KEY,
  STATE_CACHE_MATCHED_KEY,
  STATE_PYTHON_CACHE_MATCHED_KEY,
} from "./cache/restore-cache";
import { STATE_UV_PATH, STATE_UV_VERSION } from "./utils/constants";
import { loadInputs, type SetupInputs } from "./utils/inputs";
import * as log from "./utils/logging";

function formatUnexpectedFailure(error: unknown): string {
  if (error instanceof Error) {
    return error.stack ?? error.message;
  }
  return String(error);
}

function failUnexpectedly(event: string, error: unknown): never {
  core.setFailed(`${event}: ${formatUnexpectedFailure(error)}`);
  process.exit(1);
}

process.on("uncaughtException", (error) => {
  failUnexpectedly("Uncaught exception", error);
});

process.on("unhandledRejection", (reason) => {
  failUnexpectedly("Unhandled promise rejection", reason);
});

export async function run(): Promise<void> {
  try {
    const inputs = loadInputs();
    if (inputs.enableCache) {
      if (inputs.saveCache) {
        await saveCache(inputs);
      } else {
        log.info("save-cache is false. Skipping save cache step.");
      }
      // https://github.com/nodejs/node/issues/56645#issuecomment-3924958861
      await new Promise((resolve) => setTimeout(resolve, 100));

      // node will stay alive if any promises are not resolved,
      // which is a possibility if HTTP requests are dangling
      // due to retries or timeouts. We know that if we got here
      // that all promises that we care about have successfully
      // resolved, so simply exit with success.
      process.exit(0);
    }
  } catch (error) {
    const err = error as Error;
    core.setFailed(err.message);
  }
}

async function saveCache(inputs: SetupInputs): Promise<void> {
  const cacheKey = core.getState(STATE_CACHE_KEY);
  const matchedKey = core.getState(STATE_CACHE_MATCHED_KEY);

  if (!cacheKey) {
    log.warning("Error retrieving cache key from state.");
    return;
  }
  if (matchedKey === cacheKey) {
    log.info(`Cache hit occurred on key ${cacheKey}, not saving cache.`);
  } else {
    if (inputs.pruneCache) {
      await pruneCache();
    }

    const actualCachePath = getUvCachePath(inputs);
    if (!fs.existsSync(actualCachePath)) {
      if (inputs.ignoreNothingToCache) {
        log.info(
          "No cacheable uv cache paths were found. Ignoring because ignore-nothing-to-cache is enabled.",
        );
      } else {
        throw new Error(
          `Cache path ${actualCachePath} does not exist on disk. This likely indicates that there are no dependencies to cache. Consider disabling the cache input if it is not needed.`,
        );
      }
    } else {
      await saveCacheToKey(
        cacheKey,
        actualCachePath,
        STATE_CACHE_MATCHED_KEY,
        "uv cache",
      );
    }
  }

  if (inputs.cachePython) {
    if (!fs.existsSync(inputs.pythonDir)) {
      log.warning(
        `Python cache path ${inputs.pythonDir} does not exist on disk. Skipping Python cache save because no managed Python installation was found. If you want uv to install managed Python instead of using a system interpreter, set UV_PYTHON_PREFERENCE=only-managed.`,
      );
      return;
    }

    const pythonCacheKey = `${cacheKey}-python`;
    await saveCacheToKey(
      pythonCacheKey,
      inputs.pythonDir,
      STATE_PYTHON_CACHE_MATCHED_KEY,
      "Python cache",
    );
  }
}

async function pruneCache(): Promise<void> {
  const forceSupported = pep440.gte(core.getState(STATE_UV_VERSION), "0.8.24");

  const options: exec.ExecOptions = {
    silent: false,
  };
  const execArgs = ["cache", "prune", "--ci"];
  if (forceSupported) {
    execArgs.push("--force");
  }

  log.info("Pruning cache...");
  const uvPath = core.getState(STATE_UV_PATH);
  await exec.exec(uvPath, execArgs, options);
}

function getUvCachePath(inputs: SetupInputs): string {
  if (inputs.cacheLocalPath === undefined) {
    throw new Error(
      "cache-local-path is not set. Cannot save cache without a valid cache path.",
    );
  }
  if (
    process.env.UV_CACHE_DIR &&
    process.env.UV_CACHE_DIR !== inputs.cacheLocalPath.path
  ) {
    log.warning(
      `The environment variable UV_CACHE_DIR has been changed to "${process.env.UV_CACHE_DIR}", by an action or step running after astral-sh/setup-uv. This can lead to unexpected behavior. If you expected this to happen set the cache-local-path input to "${process.env.UV_CACHE_DIR}" instead of "${inputs.cacheLocalPath.path}".`,
    );
    return process.env.UV_CACHE_DIR;
  }
  return inputs.cacheLocalPath.path;
}

async function saveCacheToKey(
  cacheKey: string,
  cachePath: string,
  stateKey: string,
  cacheName: string,
): Promise<void> {
  const matchedKey = core.getState(stateKey);

  if (matchedKey === cacheKey) {
    log.info(`${cacheName} hit occurred on key ${cacheKey}, not saving cache.`);
    return;
  }

  log.info(`Including ${cacheName} path: ${cachePath}`);
  await cache.saveCache([cachePath], cacheKey);
  log.info(`${cacheName} saved with key: ${cacheKey}`);
}

run();
