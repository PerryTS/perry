import fs from "node:fs";
import os from "node:os";
import * as core from "@actions/core";
import * as exec from "@actions/exec";
export type Platform =
  | "unknown-linux-gnu"
  | "unknown-linux-musl"
  | "unknown-linux-musleabihf"
  | "apple-darwin"
  | "pc-windows-msvc";
export type Architecture =
  | "i686"
  | "x86_64"
  | "aarch64"
  | "s390x"
  | "riscv64gc"
  | "powerpc64le";

export function getArch(): Architecture | undefined {
  const arch = process.arch;
  const archMapping: { [key: string]: Architecture } = {
    arm64: "aarch64",
    ia32: "i686",
    ppc64: "powerpc64le",
    riscv64: "riscv64gc",
    s390x: "s390x",
    x64: "x86_64",
  };

  if (arch in archMapping) {
    return archMapping[arch];
  }
}

export async function getPlatform(): Promise<Platform | undefined> {
  const processPlatform = process.platform;
  const platformMapping: { [key: string]: Platform } = {
    darwin: "apple-darwin",
    linux: "unknown-linux-gnu",
    win32: "pc-windows-msvc",
  };

  if (processPlatform in platformMapping) {
    const platform = platformMapping[processPlatform];
    if (platform === "unknown-linux-gnu") {
      const isMusl = await isMuslOs();
      return isMusl ? "unknown-linux-musl" : platform;
    }
    return platform;
  }
}

async function isMuslOs(): Promise<boolean> {
  let stdOutput = "";
  let errOutput = "";
  const options: exec.ExecOptions = {
    ignoreReturnCode: true,
    listeners: {
      stderr: (data: Buffer) => {
        errOutput += data.toString();
      },
      stdout: (data: Buffer) => {
        stdOutput += data.toString();
      },
    },
    silent: !core.isDebug(),
  };

  try {
    const execArgs = ["--version"];
    await exec.exec("ldd", execArgs, options);
    return stdOutput.includes("musl") || errOutput.includes("musl");
  } catch (error) {
    const err = error as Error;
    core.warning(
      `Failed to determine glibc or musl. Falling back to glibc. Error: ${err.message}`,
    );
    return false;
  }
}

/**
 * Returns OS name and version for cache key differentiation.
 * Examples: "ubuntu-22.04", "macos-14", "windows-2022"
 * Throws if OS detection fails.
 */
export function getOSNameVersion(): string {
  const platform = process.platform;

  if (platform === "linux") {
    return getLinuxOSNameVersion();
  }
  if (platform === "darwin") {
    return getMacOSNameVersion();
  }
  if (platform === "win32") {
    return getWindowsNameVersion();
  }

  throw new Error(`Unsupported platform: ${platform}`);
}

function getLinuxOSNameVersion(): string {
  const files = ["/etc/os-release", "/usr/lib/os-release"];
  let idWithoutVersion: string | undefined;

  for (const file of files) {
    try {
      const content = fs.readFileSync(file, "utf8");
      const id = parseOsReleaseValue(content, "ID");
      const versionId = parseOsReleaseValue(content, "VERSION_ID");
      // Fallback for rolling releases (debian:unstable/testing, arch, etc.)
      // that don't have VERSION_ID but have VERSION_CODENAME or BUILD_ID
      const versionCodename = parseOsReleaseValue(content, "VERSION_CODENAME");
      const buildId = parseOsReleaseValue(content, "BUILD_ID");

      if (id && versionId) {
        return `${id}-${versionId}`;
      }
      if (id && versionCodename) {
        return `${id}-${versionCodename}`;
      }
      if (id && buildId) {
        return `${id}-${buildId}`;
      }
      // Remember the ID but keep looking: the next file might still
      // provide a version field
      if (id && idWithoutVersion === undefined) {
        idWithoutVersion = id;
      }
    } catch {
      // Try next file
    }
  }

  // Fallback for rolling releases (e.g. void) that have no version
  // field at all
  if (idWithoutVersion) {
    return idWithoutVersion;
  }

  throw new Error(
    "Failed to determine Linux distribution. " +
      "Could not read /etc/os-release or /usr/lib/os-release",
  );
}

function parseOsReleaseValue(content: string, key: string): string | undefined {
  const regex = new RegExp(`^${key}=["']?([^"'\\n]*)["']?$`, "m");
  const match = content.match(regex);
  return match?.[1];
}

function getMacOSNameVersion(): string {
  const darwinVersion = Number.parseInt(os.release().split(".")[0], 10);
  if (Number.isNaN(darwinVersion)) {
    throw new Error(`Failed to parse macOS version from: ${os.release()}`);
  }
  const macosVersion = darwinVersion - 9;
  return `macos-${macosVersion}`;
}

function getWindowsNameVersion(): string {
  const version = os.version();
  const match = version.match(/Windows(?: Server)? (\d+)/);
  if (!match) {
    throw new Error(`Failed to parse Windows version from: ${version}`);
  }
  return `windows-${match[1]}`;
}
