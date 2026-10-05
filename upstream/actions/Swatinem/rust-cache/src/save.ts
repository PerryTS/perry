import * as core from "@actions/core";
import * as exec from "@actions/exec";

import { cleanBin, cleanGit, cleanRegistry, cleanTargetDir } from "./cleanup.js";
import { CacheConfig, isCacheUpToDate } from "./config.js";
import { getCacheProvider, reportError } from "./utils.js";

process.on("uncaughtException", (e) => {
  core.error(e.message);
  if (e.stack) {
    core.error(e.stack);
  }
});

async function run() {
  const cacheProvider = await getCacheProvider();

  const save = core.getInput("save-if").toLowerCase() || "true";

  if (!(cacheProvider.cache.isFeatureAvailable() && save === "true")) {
    return;
  }

  try {
    if (isCacheUpToDate()) {
      core.info(`Cache up-to-date.`);
      return;
    }

    const config = CacheConfig.fromState();
    config.printInfo(cacheProvider);
    core.info("");

    // TODO: remove this once https://github.com/actions/toolkit/pull/553 lands
    if (process.env["RUNNER_OS"] == "macOS") {
      await macOsWorkaround();
    }

    const workspaceCrates = core.getInput("cache-workspace-crates").toLowerCase() || "false";
    const allPackages = [];
    for (const workspace of config.workspaces) {
      const packages = await workspace.getPackagesOutsideWorkspaceRoot(config.cmdFormat);
      if (workspaceCrates === "true") {
        const wsMembers = await workspace.getWorkspaceMembers(config.cmdFormat);
        packages.push(...wsMembers);
      }
      allPackages.push(...packages);
      try {
        core.info(`... Cleaning ${workspace.target} ...`);
        await cleanTargetDir(workspace.target, packages);
      } catch (e) {
        core.debug(`${(e as any).stack}`);
      }
    }

    try {
      const crates = core.getInput("cache-all-crates").toLowerCase() || "false";
      core.info(`... Cleaning cargo registry (cache-all-crates: ${crates}) ...`);
      await cleanRegistry(allPackages, crates !== "true");
    } catch (e) {
      core.debug(`${(e as any).stack}`);
    }

    if (config.cacheBin) {
      try {
        core.info(`... Cleaning cargo/bin ...`);
        await cleanBin(config.cargoBins);
      } catch (e) {
        core.debug(`${(e as any).stack}`);
      }
    }

    try {
      core.info(`... Cleaning cargo git cache ...`);
      await cleanGit(allPackages);
    } catch (e) {
      core.debug(`${(e as any).stack}`);
    }

    core.info(`... Saving cache ...`);
    // Pass a copy of cachePaths to avoid mutating the original array as reported by:
    // https://github.com/actions/toolkit/pull/1378
    // TODO: remove this once the underlying bug is fixed.
    await cacheProvider.cache.saveCache(config.cachePaths.slice(), config.cacheKey);
  } catch (e) {
    reportError(e);
  }
  process.exit();
}

run();

async function macOsWorkaround() {
  try {
    // Workaround for https://github.com/actions/cache/issues/403
    // Also see https://github.com/rust-lang/cargo/issues/8603
    await exec.exec("sudo", ["/usr/sbin/purge"], { silent: true });
  } catch {}
}
