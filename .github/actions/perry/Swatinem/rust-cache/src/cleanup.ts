import * as core from "@actions/core";
import * as io from "@actions/io";
import fs from "fs";
import path from "path";

import { CARGO_HOME } from "./config.js";
import { exists } from "./utils.js";
import { Packages } from "./workspace.js";

export async function cleanTargetDir(targetDir: string, packages: Packages, checkTimestamp = false) {
  core.debug(`cleaning target directory "${targetDir}"`);

  // remove all *files* from the profile directory
  let dir = await fs.promises.opendir(targetDir);
  for await (const dirent of dir) {
    if (dirent.isDirectory()) {
      let dirName = path.join(dir.path, dirent.name);
      // Target-triple directories do not contain Cargo's target directory
      // markers, so identify profiles by their artifact directories as well.
      const isProfile =
        dirent.name === "tests" ||
        (await exists(path.join(dirName, "build"))) ||
        (await exists(path.join(dirName, ".fingerprint"))) ||
        (await exists(path.join(dirName, "deps")));

      try {
        if (isProfile) {
          await cleanProfileTarget(dirName, packages, checkTimestamp);
        } else {
          await cleanTargetDir(dirName, packages, checkTimestamp);
        }
      } catch {}
    } else if (dirent.name !== "CACHEDIR.TAG") {
      await rm(dir.path, dirent);
    }
  }
}

async function cleanProfileTarget(profileDir: string, packages: Packages, checkTimestamp = false) {
  core.debug(`cleaning profile directory "${profileDir}"`);

  // Quite a few testing utility crates store compilation artifacts as nested
  // workspaces under `target/tests`. Notably, `target/tests/target` and
  // `target/tests/trybuild`.
  if (path.basename(profileDir) === "tests") {
    try {
      // https://github.com/vertexclique/kaos/blob/9876f6c890339741cc5be4b7cb9df72baa5a6d79/src/cargo.rs#L25
      // https://github.com/eupn/macrotest/blob/c4151a5f9f545942f4971980b5d264ebcd0b1d11/src/cargo.rs#L27
      cleanTargetDir(path.join(profileDir, "target"), packages, checkTimestamp);
    } catch {}
    try {
      // https://github.com/dtolnay/trybuild/blob/eec8ca6cb9b8f53d0caf1aa499d99df52cae8b40/src/cargo.rs#L50
      cleanTargetDir(path.join(profileDir, "trybuild"), packages, checkTimestamp);
    } catch {}

    // Delete everything else.
    await rmExcept(profileDir, new Set(["target", "trybuild"]), checkTimestamp);

    return;
  }

  let keepProfile = new Set(["build", ".fingerprint", "deps"]);
  await rmExcept(profileDir, keepProfile);

  const keepPkg = new Set(packages.flatMap((p) => [p.name, ...p.targets.map((t) => t.replace(/-/g, "_"))]));
  await rmExcept(path.join(profileDir, "build"), keepPkg, checkTimestamp);
  await rmExcept(path.join(profileDir, ".fingerprint"), keepPkg, checkTimestamp);

  const keepDeps = new Set(
    packages.flatMap((p) => {
      const names = [];
      for (const n of [p.name, ...p.targets]) {
        const name = n.replace(/-/g, "_");
        names.push(name, `lib${name}`);
      }
      return names;
    }),
  );
  await rmExcept(path.join(profileDir, "deps"), keepDeps, checkTimestamp);
}

/**
 * Clean the cargo bin directory, removing the binaries that existed
 * when the action started, as they were not created by the build.
 *
 * @param oldBins The binaries that existed when the action started.
 */
export async function cleanBin(oldBins: Array<string>) {
  const binsToRemove = new Set<string>(oldBins);

  const dir = await fs.promises.opendir(path.join(CARGO_HOME, "bin"));
  for await (const dirent of dir) {
    if (dirent.isFile() && binsToRemove.has(dirent.name)) {
      await rm(dir.path, dirent);
    }
  }
}

export async function cleanRegistry(packages: Packages, crates = true) {
  // remove `.cargo/credentials.toml`
  try {
    const credentials = path.join(CARGO_HOME, "credentials.toml");
    core.debug(`deleting "${credentials}"`);
    await fs.promises.unlink(credentials);
  } catch {}

  // `.cargo/registry/index`
  let pkgSet = new Set(packages.map((p) => p.name));
  const indexDir = await fs.promises.opendir(path.join(CARGO_HOME, "registry", "index"));
  for await (const dirent of indexDir) {
    if (dirent.isDirectory()) {
      // eg `.cargo/registry/index/github.com-1ecc6299db9ec823`
      // or `.cargo/registry/index/index.crates.io-e139d0d48fed7772`
      const dirPath = path.join(indexDir.path, dirent.name);

      // for a git registry, we can remove `.cache`, as cargo will recreate it from git
      if (await exists(path.join(dirPath, ".git"))) {
        await rmRF(path.join(dirPath, ".cache"));
      } else {
        await cleanRegistryIndexCache(dirPath, pkgSet);
      }
    }
  }

  if (!crates) {
    core.debug("skipping registry cache and src cleanup");
    return;
  }

  // `.cargo/registry/src`
  // Cargo usually re-creates these from the `.crate` cache below,
  // but for some reason that does not work for `-sys` crates that check timestamps
  // to decide if rebuilds are necessary.
  pkgSet = new Set(packages.filter((p) => p.name.endsWith("-sys")).map((p) => `${p.name}-${p.version}`));
  const srcDir = await fs.promises.opendir(path.join(CARGO_HOME, "registry", "src"));
  for await (const dirent of srcDir) {
    if (dirent.isDirectory()) {
      // eg `.cargo/registry/src/github.com-1ecc6299db9ec823`
      // or `.cargo/registry/src/index.crates.io-e139d0d48fed7772`
      const dir = await fs.promises.opendir(path.join(srcDir.path, dirent.name));
      for await (const dirent of dir) {
        if (dirent.isDirectory() && !pkgSet.has(dirent.name)) {
          await rmRF(path.join(dir.path, dirent.name));
        }
      }
    }
  }

  // `.cargo/registry/cache`
  pkgSet = new Set(packages.map((p) => `${p.name}-${p.version}.crate`));
  const cacheDir = await fs.promises.opendir(path.join(CARGO_HOME, "registry", "cache"));
  for await (const dirent of cacheDir) {
    if (dirent.isDirectory()) {
      // eg `.cargo/registry/cache/github.com-1ecc6299db9ec823`
      // or `.cargo/registry/cache/index.crates.io-e139d0d48fed7772`
      const dir = await fs.promises.opendir(path.join(cacheDir.path, dirent.name));
      for await (const dirent of dir) {
        // here we check that the downloaded `.crate` matches one from our dependencies
        if (dirent.isFile() && !pkgSet.has(dirent.name)) {
          await rm(dir.path, dirent);
        }
      }
    }
  }
}

/// Recursively walks and cleans the index `.cache`
async function cleanRegistryIndexCache(dirName: string, keepPkg: Set<string>) {
  let dirIsEmpty = true;
  const cacheDir = await fs.promises.opendir(dirName);
  for await (const dirent of cacheDir) {
    if (dirent.isDirectory()) {
      if (await cleanRegistryIndexCache(path.join(dirName, dirent.name), keepPkg)) {
        await rm(dirName, dirent);
      } else {
        dirIsEmpty &&= false;
      }
    } else {
      if (keepPkg.has(dirent.name)) {
        dirIsEmpty &&= false;
      } else {
        await rm(dirName, dirent);
      }
    }
  }
  return dirIsEmpty;
}

export async function cleanGit(packages: Packages) {
  const coPath = path.join(CARGO_HOME, "git", "checkouts");
  const dbPath = path.join(CARGO_HOME, "git", "db");
  const repos = new Map<string, Set<string>>();
  for (const p of packages) {
    if (!p.path.startsWith(coPath)) {
      continue;
    }
    const [repo, ref] = p.path.slice(coPath.length + 1).split(path.sep);
    const refs = repos.get(repo);
    if (refs) {
      refs.add(ref);
    } else {
      repos.set(repo, new Set([ref]));
    }
  }

  // we have to keep both the clone, and the checkout, removing either will
  // trigger a rebuild

  // clean the db
  try {
    let dir = await fs.promises.opendir(dbPath);
    for await (const dirent of dir) {
      if (!repos.has(dirent.name)) {
        await rm(dir.path, dirent);
      }
    }
  } catch {}

  // clean the checkouts
  try {
    let dir = await fs.promises.opendir(coPath);
    for await (const dirent of dir) {
      const refs = repos.get(dirent.name);
      if (!refs) {
        await rm(dir.path, dirent);
        continue;
      }
      if (!dirent.isDirectory()) {
        continue;
      }
      const refsDir = await fs.promises.opendir(path.join(dir.path, dirent.name));
      for await (const dirent of refsDir) {
        if (!refs.has(dirent.name)) {
          await rm(refsDir.path, dirent);
        }
      }
    }
  } catch {}
}

const ONE_WEEK = 7 * 24 * 3600 * 1000;

/**
 * Removes all files or directories in `dirName` matching some criteria.
 *
 * When the `checkTimestamp` flag is set, this will also remove anything older
 * than one week.
 *
 * Otherwise, it will remove everything that does not match any string in the
 * `keepPrefix` set.
 * The matching strips and trailing `-$hash` suffix.
 *
 * Cargo's newer `build-dir` layout (rust-lang/cargo#17258) nests the hash as
 * a subdirectory instead of appending it, so entries there are bare package
 * names with no suffix to strip. Check for an exact match first so those
 * names (which may themselves contain hyphens) aren't mistaken for a
 * `<name>-<hash>` entry from the old layout and truncated incorrectly.
 */
async function rmExcept(dirName: string, keepPrefix: Set<string>, checkTimestamp = false) {
  const dir = await fs.promises.opendir(dirName);
  for await (const dirent of dir) {
    if (checkTimestamp) {
      const fileName = path.join(dir.path, dirent.name);
      const { mtime } = await fs.promises.stat(fileName);
      const isOutdated = Date.now() - mtime.getTime() > ONE_WEEK;

      if (isOutdated) {
        await rm(dir.path, dirent);
      }
      return;
    }

    let name = dirent.name;

    // in Cargo's V1 layout, all packages are suffixed by their hash.
    // in V2, all package hashes are subdirectories instead.
    // Check both possible naming standards for packages and accept either.

    // v2 package format
    if (!keepPrefix.has(name)) {
      // now check for v1 package format
      // strip the trailing hash
      const idx = name.lastIndexOf("-");
      if (idx !== -1) {
        name = name.slice(0, idx);
      }
      if (!keepPrefix.has(name)) {
        await rm(dir.path, dirent);
      }
    }
  }
}

async function rm(parent: string, dirent: fs.Dirent) {
  try {
    const fileName = path.join(parent, dirent.name);
    core.debug(`deleting "${fileName}"`);
    if (dirent.isFile()) {
      await fs.promises.unlink(fileName);
    } else if (dirent.isDirectory()) {
      await io.rmRF(fileName);
    }
  } catch {}
}

async function rmRF(dirName: string) {
  core.debug(`deleting "${dirName}"`);
  await io.rmRF(dirName);
}
