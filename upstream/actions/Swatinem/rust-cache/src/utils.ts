import * as core from "@actions/core";
import * as exec from "@actions/exec";
import fs from "fs";

export function reportError(e: any) {
  const { commandFailed } = e;
  if (commandFailed) {
    core.error(`Command failed: ${commandFailed.command}`);
    core.error(commandFailed.stderr);
  } else {
    core.error(`${e.stack}`);
  }
}

export async function getCmdOutput(cmdFormat: string, cmd: string, options: exec.ExecOptions = {}): Promise<string> {
  cmd = cmdFormat.replace("{0}", cmd);
  let stdout = "";
  let stderr = "";
  try {
    await exec.exec(cmd, [], {
      silent: true,
      listeners: {
        stdout(data) {
          stdout += data.toString();
        },
        stderr(data) {
          stderr += data.toString();
        },
      },
      ...options,
    });
  } catch (e) {
    (e as any).commandFailed = {
      command: cmd,
      stderr,
    };
    throw e;
  }
  return stdout;
}

export interface GhCache {
  isFeatureAvailable: typeof import("@actions/cache").isFeatureAvailable;
  restoreCache: typeof import("@actions/cache").restoreCache;
  saveCache: (paths: string[], key: string) => Promise<string | number>;
}

export interface CacheProvider {
  name: string;
  cache: GhCache;
}

export async function getCacheProvider(): Promise<CacheProvider> {
  const cacheProvider = core.getInput("cache-provider");
  let cache: GhCache;
  switch (cacheProvider) {
    case "github":
      cache = await import("@actions/cache");
      break;
    case "warpbuild":
      cache = await import("@actions/warpbuild-cache");
      break;
    default:
      throw new Error(`The \`cache-provider\` \`${cacheProvider}\` is not valid.`);
  }

  return {
    name: cacheProvider,
    cache: cache,
  };
}

export async function exists(path: string) {
  try {
    await fs.promises.access(path);
    return true;
  } catch {
    return false;
  }
}
