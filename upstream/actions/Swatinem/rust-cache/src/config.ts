import * as core from "@actions/core";
import * as glob from "@actions/glob";
import crypto from "crypto";
import fs from "fs/promises";
import { createReadStream } from "fs";
import os from "os";
import path from "path";
import * as toml from "smol-toml";

import { CacheProvider, exists, getCmdOutput } from "./utils.js";
import { Workspace } from "./workspace.js";

const HOME = os.homedir();
export const CARGO_HOME = process.env.CARGO_HOME || path.join(HOME, ".cargo");

const STATE_CONFIG = "RUST_CACHE_CONFIG";
const HASH_LENGTH = 8;

export class CacheConfig {
  /** A format string for running commands */
  public cmdFormat: string = "";

  /** All the paths we want to cache */
  public cachePaths: Array<string> = [];
  /** The primary cache key */
  public cacheKey = "";
  /** The secondary (restore) key that only contains the prefix and environment */
  public restoreKey = "";

  /** Whether to cache CARGO_HOME/.bin */
  public cacheBin: boolean = true;

  /** The workspace configurations */
  public workspaces: Array<Workspace> = [];

  /** The cargo binaries present during main step */
  public cargoBins: Array<string> = [];

  /** The prefix portion of the cache key */
  private keyPrefix = "";
  /** The rust version considered for the cache key */
  private keyRust: Array<string> = [];
  /** The environment variables considered for the cache key */
  private keyEnvs: Array<string> = [];
  /** The files considered for the cache key */
  private keyFiles: Array<string> = [];

  private constructor() {}

  /**
   * Constructs a [`CacheConfig`] with all the paths and keys.
   *
   * This will read the action `input`s, and read and persist `state` as necessary.
   */
  static async new(): Promise<CacheConfig> {
    const self = new CacheConfig();

    let cmdFormat = core.getInput("cmd-format");
    if (cmdFormat) {
      const placeholderMatches = cmdFormat.match(/\{0\}/g);
      if (!placeholderMatches || placeholderMatches.length !== 1) {
        cmdFormat = "{0}";
      }
    } else {
      cmdFormat = "{0}";
    }
    self.cmdFormat = cmdFormat;

    // Construct key prefix:
    // This uses either the `shared-key` input,
    // or the `key` input combined with the `job` key.

    let key = core.getInput("prefix-key") || "v0-rust";

    const sharedKey = core.getInput("shared-key");
    if (sharedKey) {
      key += `-${sharedKey}`;
    } else {
      const inputKey = core.getInput("key");
      if (inputKey) {
        key += `-${inputKey}`;
      }

      const job = process.env.GITHUB_JOB;
      if (job && core.getInput("add-job-id-key").toLowerCase() == "true") {
        key += `-${job}`;
      }
    }

    // Add runner OS and CPU architecture to the key to avoid cross-contamination of cache
    const runnerOS = os.type();
    const runnerArch = os.arch();
    key += `-${runnerOS}-${runnerArch}`;

    self.keyPrefix = key;

    // Construct environment portion of the key:
    // This consists of a hash that considers the rust version
    // as well as all the environment variables as given by a default list
    // and the `env-vars` input.
    // The env vars are sorted, matched by prefix and hashed into the
    // resulting environment hash.

    let hasher = crypto.createHash("sha1");
    const rustVersions = (self.keyRust = await getRustVersions(cmdFormat));
    for (const rustVersion of rustVersions) {
      hasher.update(rustVersion);
    }

    // these prefixes should cover most of the compiler / rust / cargo keys
    const envPrefixes = ["CARGO", "CC", "CFLAGS", "CXX", "CMAKE", "RUST"];
    envPrefixes.push(...core.getInput("env-vars").split(/\s+/).filter(Boolean));

    // sort the available env vars so we have a more stable hash
    const keyEnvs = [];
    const envKeys = Object.keys(process.env);
    envKeys.sort((a, b) => a.localeCompare(b));
    for (const key of envKeys) {
      const value = process.env[key];
      if (envPrefixes.some((prefix) => key.startsWith(prefix)) && value) {
        hasher.update(`${key}=${value}`);
        keyEnvs.push(key);
      }
    }

    self.keyEnvs = keyEnvs;

    // Add job hash suffix if 'add-rust-environment-hash-key' is true
    if (core.getInput("add-rust-environment-hash-key").toLowerCase() == "true") {
      key += `-${digest(hasher)}`;
    }

    self.restoreKey = key;

    // Construct the lockfiles portion of the key:
    // This considers all the files found via globbing for various manifests
    // and lockfiles.

    self.cacheBin = core.getInput("cache-bin").toLowerCase() == "true";

    // Constructs the workspace config and paths to restore:
    // The workspaces are given using a `$workspace -> $target` syntax.

    const workspaces: Array<Workspace> = [];
    const workspacesInput = core.getInput("workspaces") || ".";
    for (const workspace of workspacesInput.trim().split("\n")) {
      let [root, target = "target"] = workspace.split("->").map((s) => s.trim());
      root = path.resolve(root);
      target = path.join(root, target);
      workspaces.push(new Workspace(root, target));
    }
    self.workspaces = workspaces;

    // Add hash suffix of all rust environment lockfiles + manifests if
    // 'add-rust-environment-hash-key' is true
    if (core.getInput("add-rust-environment-hash-key").toLowerCase() == "true") {
      let keyFiles = await globFiles(".cargo/config.toml\nrust-toolchain\nrust-toolchain.toml");
      const parsedKeyFiles = []; // keyFiles that are parsed, pre-processed and hashed

      hasher = crypto.createHash("sha1");

      for (const workspace of workspaces) {
        const root = workspace.root;
        keyFiles.push(
          ...(await globFiles(
            `${root}/**/.cargo/config.toml\n${root}/**/rust-toolchain\n${root}/**/rust-toolchain.toml`,
          )),
        );

        const workspaceMembers = await workspace.getWorkspaceMembers(cmdFormat);

        const cargo_manifests = sort_and_uniq(workspaceMembers.map((member) => path.join(member.path, "Cargo.toml")));

        for (const cargo_manifest of cargo_manifests) {
          try {
            const content = await fs.readFile(cargo_manifest, { encoding: "utf8" });
            // Use any since TomlPrimitive is not exposed
            const parsed = toml.parse(content) as { [key: string]: any };

            if ("package" in parsed) {
              const pack = parsed.package;
              if ("version" in pack) {
                pack["version"] = "0.0.0";
              }
            }

            for (const prefix of ["", "build-", "dev-"]) {
              const section_name = `${prefix}dependencies`;
              if (!(section_name in parsed)) {
                continue;
              }
              const deps = parsed[section_name];

              for (const key of Object.keys(deps)) {
                const dep = deps[key];

                try {
                  if ("path" in dep) {
                    dep.version = "0.0.0";
                    dep.path = "";
                  }
                } catch (_e) {
                  // Not an object, probably a string (version),
                  // continue.
                  continue;
                }
              }
            }

            hasher.update(JSON.stringify(parsed));

            parsedKeyFiles.push(cargo_manifest);
          } catch (e) {
            // Fallback to caching them as regular file
            core.warning(`Error parsing Cargo.toml manifest, fallback to caching entire file: ${e}`);
            keyFiles.push(cargo_manifest);
          }
        }

        const cargo_lock = path.join(workspace.root, "Cargo.lock");
        if (await exists(cargo_lock)) {
          try {
            const content = await fs.readFile(cargo_lock, { encoding: "utf8" });
            const parsed = toml.parse(content);

            if ((parsed.version !== 3 && parsed.version !== 4) || !("package" in parsed)) {
              // Fallback to caching them as regular file since this action
              // can only handle Cargo.lock format version 3
              core.warning("Unsupported Cargo.lock format, fallback to caching entire file");
              keyFiles.push(cargo_lock);
              continue;
            }

            // Package without `[[package]].source` and `[[package]].checksum`
            // are the one with `path = "..."` to crates within the workspace.
            const packages = (parsed.package as any[]).filter((p: any) => "source" in p || "checksum" in p);

            hasher.update(JSON.stringify(packages));

            parsedKeyFiles.push(cargo_lock);
          } catch (e) {
            // Fallback to caching them as regular file
            core.warning(`Error parsing Cargo.lock manifest, fallback to caching entire file: ${e}`);
            keyFiles.push(cargo_lock);
          }
        }
      }
      keyFiles = sort_and_uniq(keyFiles);

      for (const file of keyFiles) {
        for await (const chunk of createReadStream(file)) {
          hasher.update(chunk);
        }
      }

      keyFiles.push(...parsedKeyFiles);
      self.keyFiles = sort_and_uniq(keyFiles);

      let lockHash = digest(hasher);
      key += `-${lockHash}`;
    }

    self.cacheKey = key;

    self.cachePaths = [path.join(CARGO_HOME, "registry"), path.join(CARGO_HOME, "git")];
    if (self.cacheBin) {
      self.cachePaths = [
        path.join(CARGO_HOME, "bin"),
        path.join(CARGO_HOME, ".crates.toml"),
        path.join(CARGO_HOME, ".crates2.json"),
        ...self.cachePaths,
      ];
    }
    const cacheTargets = core.getInput("cache-targets").toLowerCase() || "true";
    if (cacheTargets === "true") {
      self.cachePaths.push(...workspaces.map((ws) => ws.target));
    }

    const cacheDirectories = core.getInput("cache-directories");
    for (const dir of cacheDirectories.trim().split(/\s+/).filter(Boolean)) {
      self.cachePaths.push(dir);
    }

    const bins = await getCargoBins();
    self.cargoBins = Array.from(bins.values());

    return self;
  }

  /**
   * Reads and returns the cache config from the action `state`.
   *
   * @throws {Error} if the state is not present.
   * @returns {CacheConfig} the configuration.
   * @see {@link CacheConfig#saveState}
   * @see {@link CacheConfig#new}
   */
  static fromState(): CacheConfig {
    const source = core.getState(STATE_CONFIG);
    if (!source) {
      throw new Error("Cache configuration not found in state");
    }

    const self = new CacheConfig();
    Object.assign(self, JSON.parse(source));
    self.workspaces = self.workspaces.map((w: any) => new Workspace(w.root, w.target));

    return self;
  }

  /**
   * Prints the configuration to the action log.
   */
  printInfo(cacheProvider: CacheProvider) {
    core.startGroup("Cache Configuration");
    core.info(`Cache Provider:`);
    core.info(`    ${cacheProvider.name}`);
    core.info(`Workspaces:`);
    for (const workspace of this.workspaces) {
      core.info(`    ${workspace.root}`);
    }
    core.info(`Cache Paths:`);
    for (const path of this.cachePaths) {
      core.info(`    ${path}`);
    }
    core.info(`Restore Key:`);
    core.info(`    ${this.restoreKey}`);
    core.info(`Cache Key:`);
    core.info(`    ${this.cacheKey}`);
    core.info(`.. Prefix:`);
    core.info(`  - ${this.keyPrefix}`);
    core.info(`.. Environment considered:`);
    core.info(`  - Rust Versions:`);
    for (const rust of this.keyRust) {
      core.info(`    - ${rust}`);
    }
    for (const env of this.keyEnvs) {
      core.info(`  - ${env}`);
    }
    core.info(`.. Lockfiles considered:`);
    for (const file of this.keyFiles) {
      core.info(`  - ${file}`);
    }
    core.endGroup();
  }

  /**
   * Saves the configuration to the state store.
   * This is used to restore the configuration in the post action.
   */
  saveState() {
    core.saveState(STATE_CONFIG, this);
  }
}

/**
 * Checks if the cache is up to date.
 *
 * @returns `true` if the cache is up to date, `false` otherwise.
 */
export function isCacheUpToDate(): boolean {
  return core.getState(STATE_CONFIG) === "";
}

/**
 * Returns a hex digest of the given hasher truncated to `HASH_LENGTH`.
 *
 * @param hasher The hasher to digest.
 * @returns The hex digest.
 */
function digest(hasher: crypto.Hash): string {
  return hasher.digest("hex").substring(0, HASH_LENGTH);
}

export async function getCargoBins(): Promise<Set<string>> {
  const bins = new Set<string>();

  try {
    const dir = await fs.opendir(path.join(CARGO_HOME, "bin"));
    for await (const dirent of dir) {
      if (dirent.isFile()) {
        bins.add(dirent.name);
      }
    }
  } catch {}

  return bins;
}

async function getRustVersions(cmdFormat: string): Promise<Array<string>> {
  const versions = new Set<string>();

  versions.add(parseRustVersion(await getCmdOutput(cmdFormat, "rustc -vV")));

  const stdout = await (async () => {
    try {
      return await getCmdOutput(cmdFormat, "rustup toolchain list --quiet");
    } catch (e) {
      core.warning(`Error running rustup toolchain list, falling back to default toolchain only: ${e}`);
      return undefined;
    }
  })();
  if (stdout !== undefined) {
    for (const toolchain of stdout.split(/[\n\r]+/)) {
      const trimmed = toolchain.trim();
      if (!trimmed) {
        continue;
      }
      versions.add(parseRustVersion(await getCmdOutput(cmdFormat, `rustup run ${toolchain} rustc -vV`)));
    }
  }
  const rustVersions = Array.from(versions);
  // Doesn't matter how they're sorted, just as long as it's deterministic.
  rustVersions.sort();
  return rustVersions;
}

interface RustVersion {
  host: string;
  release: string;
  "commit-hash": string;
}
function parseRustVersion(stdout: string): string {
  const splits = stdout
    .split(/[\n\r]+/)
    .filter(Boolean)
    .map((s) => s.split(":").map((s) => s.trim()))
    .filter((s) => s.length === 2);
  const { release, host, "commit-hash": commitHash } = Object.fromEntries(splits) as RustVersion;
  return `${release} ${host} ${commitHash}`;
}

async function globFiles(pattern: string): Promise<string[]> {
  const globber = await glob.create(pattern, {
    followSymbolicLinks: false,
  });
  // fs.stat resolve the symbolic link and returns stat for the
  // file it pointed to, so isFile would make sure the resolved
  // file is actually a regular file.
  const files = [];
  for (const file of await globber.glob()) {
    const stats = await fs.stat(file);
    if (stats.isFile()) {
      files.push(file);
    }
  }
  return files;
}

function sort_and_uniq(a: string[]) {
  return a
    .sort((a, b) => a.localeCompare(b))
    .reduce((accumulator: string[], currentValue: string) => {
      const len = accumulator.length;
      // If accumulator is empty or its last element != currentValue
      // Since array is already sorted, elements with the same value
      // are grouped together to be continugous in space.
      //
      // If currentValue != last element, then it must be unique.
      if (len == 0 || accumulator[len - 1].localeCompare(currentValue) != 0) {
        accumulator.push(currentValue);
      }
      return accumulator;
    }, []);
}
