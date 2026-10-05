import fs from "node:fs";
import path from "node:path";
import * as core from "@actions/core";
import { getPythonVersionFromToolVersions } from "../version/tool-versions-file";
import { getConfigValueFromTomlFile } from "./config-file";
import * as log from "./logging";

export enum CacheLocalSource {
  Input,
  Config,
  Env,
  Default,
}

export interface CacheLocalPath {
  path: string;
  source: CacheLocalSource;
}

export type ResolutionStrategy = "highest" | "lowest";

export interface SetupInputs {
  workingDirectory: string;
  version: string;
  versionFile: string;
  pythonVersion: string;
  activateEnvironment: boolean;
  noProject: boolean;
  venvPath: string;
  checksum: string;
  enableCache: boolean;
  restoreCache: boolean;
  saveCache: boolean;
  cacheSuffix: string;
  cacheLocalPath?: CacheLocalPath;
  cacheDependencyGlob: string;
  pruneCache: boolean;
  cachePython: boolean;
  ignoreNothingToCache: boolean;
  ignoreEmptyWorkdir: boolean;
  toolBinDir?: string;
  toolDir?: string;
  pythonDir: string;
  githubToken: string;
  manifestFile?: string;
  downloadFromAstralMirror: boolean;
  addProblemMatchers: boolean;
  quiet: boolean;
  resolutionStrategy: ResolutionStrategy;
}

export function loadInputs(): SetupInputs {
  const workingDirectory = core.getInput("working-directory");
  const version = core.getInput("version");
  const versionFile = getVersionFile(workingDirectory);
  const pythonVersion = getPythonVersion(versionFile);
  const activateEnvironment = core.getBooleanInput("activate-environment");
  const noProject = core.getBooleanInput("no-project");
  const venvPath = getVenvPath(workingDirectory, activateEnvironment);
  const checksum = core.getInput("checksum");
  const enableCache = getEnableCache();
  const restoreCache = core.getInput("restore-cache") === "true";
  const saveCache = getSaveCache();
  const cacheSuffix = core.getInput("cache-suffix") || "";
  const cacheLocalPath = getCacheLocalPath(
    workingDirectory,
    versionFile,
    enableCache,
  );
  const cacheDependencyGlob = getCacheDependencyGlob(workingDirectory);
  const pruneCache = core.getInput("prune-cache") === "true";
  const cachePython = core.getInput("cache-python") === "true";
  const ignoreNothingToCache =
    core.getInput("ignore-nothing-to-cache") === "true";
  const ignoreEmptyWorkdir = core.getInput("ignore-empty-workdir") === "true";
  const toolBinDir = getToolBinDir(workingDirectory);
  const toolDir = getToolDir(workingDirectory);
  const pythonDir = getUvPythonDir();
  const githubToken = core.getInput("github-token");
  const manifestFile = getManifestFile();
  const downloadFromAstralMirror =
    core.getInput("download-from-astral-mirror") === "true";
  const addProblemMatchers = core.getInput("add-problem-matchers") === "true";
  const quiet = core.getInput("quiet") === "true";
  const resolutionStrategy = getResolutionStrategy();

  return {
    activateEnvironment,
    addProblemMatchers,
    cacheDependencyGlob,
    cacheLocalPath,
    cachePython,
    cacheSuffix,
    checksum,
    downloadFromAstralMirror,
    enableCache,
    githubToken,
    ignoreEmptyWorkdir,
    ignoreNothingToCache,
    manifestFile,
    noProject,
    pruneCache,
    pythonDir,
    pythonVersion,
    quiet,
    resolutionStrategy,
    restoreCache,
    saveCache,
    toolBinDir,
    toolDir,
    venvPath,
    version,
    versionFile,
    workingDirectory,
  };
}

function getVersionFile(workingDirectory: string): string {
  const versionFileInput = core.getInput("version-file");
  if (versionFileInput !== "") {
    const tildeExpanded = expandTilde(versionFileInput);
    return resolveRelativePath(workingDirectory, tildeExpanded);
  }
  return versionFileInput;
}

function getPythonVersion(versionFile: string): string {
  const pythonVersionInput = core.getInput("python-version");
  if (pythonVersionInput !== "") {
    return pythonVersionInput;
  }
  if (process.env.UV_PYTHON !== undefined && process.env.UV_PYTHON !== "") {
    return "";
  }
  if (versionFile === "" || !fs.existsSync(versionFile)) {
    return "";
  }

  try {
    return getPythonVersionFromToolVersions(versionFile) ?? "";
  } catch (err) {
    log.warning(
      `Error while parsing Python version from ${versionFile}: ${(err as Error).message}`,
    );
    return "";
  }
}

function getVenvPath(
  workingDirectory: string,
  activateEnvironment: boolean,
): string {
  const venvPathInput = core.getInput("venv-path");
  if (venvPathInput !== "") {
    if (!activateEnvironment) {
      log.warning("venv-path is only used when activate-environment is true");
    }
    const tildeExpanded = expandTilde(venvPathInput);
    return normalizePath(resolveRelativePath(workingDirectory, tildeExpanded));
  }
  return normalizePath(resolveRelativePath(workingDirectory, ".venv"));
}

function getEnableCache(): boolean {
  const enableCacheInput = core.getInput("enable-cache");
  if (enableCacheInput === "auto") {
    if (process.env.RUNNER_ENVIRONMENT !== "github-hosted") {
      return false;
    }

    const eventName = process.env.GITHUB_EVENT_NAME;
    const isTagPush =
      eventName === "push" && process.env.GITHUB_REF?.startsWith("refs/tags/");
    if (isTagPush) {
      log.info("Caching is disabled for tag pushes");
      return false;
    }
    if (
      eventName === "pull_request_target" ||
      eventName === "workflow_run" ||
      eventName === "release"
    ) {
      log.info(`Caching is disabled for the ${eventName} event`);
      return false;
    }

    return true;
  }
  return enableCacheInput === "true";
}

function getSaveCache(): boolean {
  const saveCacheInput = core.getInput("save-cache");
  if (saveCacheInput === "auto") {
    if (process.env.GITHUB_EVENT_NAME === "merge_group") {
      log.info("Cache saving is disabled for the merge_group event");
      return false;
    }
    return true;
  }
  return saveCacheInput === "true";
}

function getToolBinDir(workingDirectory: string): string | undefined {
  const toolBinDirInput = core.getInput("tool-bin-dir");
  if (toolBinDirInput !== "") {
    const tildeExpanded = expandTilde(toolBinDirInput);
    return resolveRelativePath(workingDirectory, tildeExpanded);
  }
  if (process.platform === "win32") {
    if (process.env.RUNNER_TEMP !== undefined) {
      return `${process.env.RUNNER_TEMP}${path.sep}uv-tool-bin-dir`;
    }
    throw Error(
      "Could not determine UV_TOOL_BIN_DIR. Please make sure RUNNER_TEMP is set or provide the tool-bin-dir input",
    );
  }
  return undefined;
}

function getToolDir(workingDirectory: string): string | undefined {
  const toolDirInput = core.getInput("tool-dir");
  if (toolDirInput !== "") {
    const tildeExpanded = expandTilde(toolDirInput);
    return resolveRelativePath(workingDirectory, tildeExpanded);
  }
  if (process.platform === "win32") {
    if (process.env.RUNNER_TEMP !== undefined) {
      return `${process.env.RUNNER_TEMP}${path.sep}uv-tool-dir`;
    }
    throw Error(
      "Could not determine UV_TOOL_DIR. Please make sure RUNNER_TEMP is set or provide the tool-dir input",
    );
  }
  return undefined;
}

function getCacheLocalPath(
  workingDirectory: string,
  versionFile: string,
  enableCache: boolean,
): CacheLocalPath | undefined {
  const cacheLocalPathInput = core.getInput("cache-local-path");
  if (cacheLocalPathInput !== "") {
    const tildeExpanded = expandTilde(cacheLocalPathInput);
    return {
      path: resolveRelativePath(workingDirectory, tildeExpanded),
      source: CacheLocalSource.Input,
    };
  }
  const cacheDirFromConfig = getCacheDirFromConfig(
    workingDirectory,
    versionFile,
  );
  if (cacheDirFromConfig !== undefined) {
    return { path: cacheDirFromConfig, source: CacheLocalSource.Config };
  }
  if (process.env.UV_CACHE_DIR !== undefined) {
    log.info(`UV_CACHE_DIR is already set to ${process.env.UV_CACHE_DIR}`);
    return { path: process.env.UV_CACHE_DIR, source: CacheLocalSource.Env };
  }
  if (enableCache) {
    if (process.env.RUNNER_ENVIRONMENT === "github-hosted") {
      if (process.env.RUNNER_TEMP !== undefined) {
        return {
          path: `${process.env.RUNNER_TEMP}${path.sep}setup-uv-cache`,
          source: CacheLocalSource.Default,
        };
      }
      throw Error(
        "Could not determine UV_CACHE_DIR. Please make sure RUNNER_TEMP is set or provide the cache-local-path input",
      );
    }
    if (process.platform === "win32") {
      return {
        path: `${process.env.APPDATA}${path.sep}uv${path.sep}cache`,
        source: CacheLocalSource.Default,
      };
    }
    return {
      path: `${process.env.HOME}${path.sep}.cache${path.sep}uv`,
      source: CacheLocalSource.Default,
    };
  }
}

function getCacheDirFromConfig(
  workingDirectory: string,
  versionFile: string,
): string | undefined {
  for (const filePath of [versionFile, "uv.toml", "pyproject.toml"]) {
    const resolvedPath = resolveRelativePath(workingDirectory, filePath);
    try {
      const cacheDir = getConfigValueFromTomlFile(resolvedPath, "cache-dir");
      if (cacheDir !== undefined) {
        log.info(`Found cache-dir in ${resolvedPath}: ${cacheDir}`);
        return cacheDir;
      }
    } catch (err) {
      const message = (err as Error).message;
      log.warning(`Error while parsing ${filePath}: ${message}`);
      return undefined;
    }
  }
  return undefined;
}

export function getUvPythonDir(): string {
  if (process.env.UV_PYTHON_INSTALL_DIR !== undefined) {
    log.info(
      `UV_PYTHON_INSTALL_DIR is already set to ${process.env.UV_PYTHON_INSTALL_DIR}`,
    );
    return process.env.UV_PYTHON_INSTALL_DIR;
  }
  if (process.env.RUNNER_ENVIRONMENT !== "github-hosted") {
    if (process.platform === "win32") {
      return `${process.env.APPDATA}${path.sep}uv${path.sep}python`;
    }
    return `${process.env.HOME}${path.sep}.local${path.sep}share${path.sep}uv${path.sep}python`;
  }
  if (process.env.RUNNER_TEMP !== undefined) {
    return `${process.env.RUNNER_TEMP}${path.sep}uv-python-dir`;
  }
  throw Error(
    "Could not determine UV_PYTHON_INSTALL_DIR. Please make sure RUNNER_TEMP is set or provide the UV_PYTHON_INSTALL_DIR environment variable",
  );
}

function getCacheDependencyGlob(workingDirectory: string): string {
  const cacheDependencyGlobInput = core.getInput("cache-dependency-glob");
  if (cacheDependencyGlobInput !== "") {
    return cacheDependencyGlobInput
      .split("\n")
      .map((part) => part.trim())
      .map((part) => expandTilde(part))
      .map((part) => resolveRelativePath(workingDirectory, part))
      .join("\n");
  }
  return cacheDependencyGlobInput;
}

function expandTilde(input: string): string {
  if (input.startsWith("~")) {
    return `${process.env.HOME}${input.substring(1)}`;
  }
  return input;
}

function normalizePath(inputPath: string): string {
  const normalized = path.normalize(inputPath);
  const root = path.parse(normalized).root;

  // Remove any trailing path separators, except when the whole path is the root.
  let trimmed = normalized;
  while (trimmed.length > root.length && trimmed.endsWith(path.sep)) {
    trimmed = trimmed.slice(0, -1);
  }

  return trimmed;
}

function resolveRelativePath(
  workingDirectory: string,
  inputPath: string,
): string {
  const hasNegation = inputPath.startsWith("!");
  const pathWithoutNegation = hasNegation ? inputPath.substring(1) : inputPath;

  const resolvedPath = path.resolve(workingDirectory, pathWithoutNegation);

  core.debug(
    `Resolving relative path ${inputPath} to ${hasNegation ? "!" : ""}${resolvedPath}`,
  );
  return hasNegation ? `!${resolvedPath}` : resolvedPath;
}

function getManifestFile(): string | undefined {
  const manifestFileInput = core.getInput("manifest-file");
  if (manifestFileInput !== "") {
    return manifestFileInput;
  }
  return undefined;
}

function getResolutionStrategy(): ResolutionStrategy {
  const resolutionStrategyInput = core.getInput("resolution-strategy");
  if (resolutionStrategyInput === "lowest") {
    return "lowest";
  }
  if (resolutionStrategyInput === "highest" || resolutionStrategyInput === "") {
    return "highest";
  }
  throw new Error(
    `Invalid resolution-strategy: ${resolutionStrategyInput}. Must be 'highest' or 'lowest'.`,
  );
}
