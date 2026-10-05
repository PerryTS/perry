import { debug, warning } from "@actions/core";
import { info } from "node:console";
import { createHash } from "node:crypto";
import { existsSync, readFileSync, renameSync } from "node:fs";
import { resolve, basename } from "node:path";
import { compareVersions, validate } from "compare-versions";

// First Bun version that ships native Windows ARM64 binaries.
const WINDOWS_ARM64_MIN_VERSION = "1.3.10";

export function getCacheKey(url: string): string {
  return `bun-${createHash("sha1").update(url).digest("base64")}`;
}

export function extractVersionFromUrl(url: string): string | undefined {
  const match = url.match(/\/bun-v([^/]+)\//);
  return match?.[1];
}

export async function request(
  url: string,
  init?: RequestInit,
): Promise<Response> {
  const headers = new Headers(init?.headers);
  if (!headers.has("User-Agent")) {
    headers.set("User-Agent", "@oven-sh/setup-bun");
  }

  const res = await fetch(url, {
    ...init,
    headers,
  });
  if (!res.ok) {
    const body = await res.text().catch(() => "");
    throw new Error(
      `Failed to fetch url ${url}. (status code: ${res.status}, status text: ${res.statusText})${body ? `\n${body}` : ""}`,
    );
  }

  return res;
}

export function addExtension(path: string, ext: string): string {
  if (!path.endsWith(ext)) {
    renameSync(path, path + ext);
    return path + ext;
  }

  return path;
}

export function getPlatform(): string {
  const platform = process.platform;
  if (platform === "win32") return "windows";

  return platform;
}

export function hasNativeWindowsArm64(version?: string): boolean {
  if (!version) return false;
  const cleaned = version.replace(/^bun-v/, "");
  // Non-semver versions like "canary" represent latest builds which ship ARM64.
  if (!validate(cleaned)) return true;
  return compareVersions(cleaned, WINDOWS_ARM64_MIN_VERSION) >= 0;
}

export function getArchitecture(
  os: string,
  arch: string,
  version?: string,
): string {
  if (os === "windows" && (arch === "aarch64" || arch === "arm64")) {
    if (!hasNativeWindowsArm64(version)) {
      warning(
        [
          "⚠️ This version of Bun does not provide native arm64 builds for Windows.",
          "Using x64 baseline build which will run through Microsoft's x64 emulation layer.",
          "This may result in reduced performance and potential compatibility issues.",
          "💡 For best performance, consider using Bun >= 1.3.10, x64 Windows runners, or other platforms with native support.",
        ].join("\n"),
      );

      return "x64";
    }
  }

  if (arch === "arm64") return "aarch64";
  return arch;
}

export function getAvx2(
  os: string,
  arch: string,
  avx2?: boolean,
  version?: string,
): boolean {
  // Workaround for absence of arm64 builds on Windows before 1.3.10 (#130)
  if (os === "windows" && (arch === "aarch64" || arch === "arm64")) {
    if (!hasNativeWindowsArm64(version)) {
      return false;
    }
    // Native ARM64 builds don't use AVX2 suffix
    return true;
  }

  // Check AVX2 support on x64 Linux
  if (os === "linux" && arch === "x64" && avx2 === undefined) {
    try {
      const cpuInfo = readFileSync("/proc/cpuinfo", "utf8");
      const hasAvx2 = cpuInfo.includes("avx2");
      return hasAvx2;
    } catch (error) {
      warning(`Failed to detect AVX2 support.`);
      return false;
    }
  }

  return avx2 ?? true;
}

const FILE_VERSION_READERS = {
  "package.json": (content: string) => {
    const pkg = JSON.parse(content);
    return pkg.packageManager?.split("bun@")?.[1] ?? pkg.engines?.bun;
  },
  ".tool-versions": (content: string) =>
    content.match(/^bun\s*(?<version>.*?)$/m)?.groups?.version,
  ".bumrc": (content: string) => content, // https://github.com/owenizedd/bum
  ".bun-version": (content: string) => content,
};

export function readVersionFromFile(
  file: string,
  silent = false,
): string | undefined {
  const cwd = process.env.GITHUB_WORKSPACE;
  if (!cwd) {
    return;
  }

  if (!file) {
    return;
  }

  debug(`Reading version from ${file}`);

  const path = resolve(cwd, file);
  const base = basename(file);

  if (!existsSync(path)) {
    if (!silent) {
      warning(`File ${path} not found`);
    }
    return;
  }

  const reader = FILE_VERSION_READERS[base] ?? (() => undefined);

  let output: string | undefined;
  try {
    output = reader(readFileSync(path, "utf8"))?.trim();

    if (!output) {
      if (!silent) {
        warning(`Failed to read version from ${file}`);
      }
      return;
    }
  } catch (error) {
    const { message } = error as Error;
    if (!silent) {
      warning(`Failed to read ${file}: ${message}`);
    }
  } finally {
    if (output) {
      info(`Obtained version ${output} from ${file}`);
      return output;
    }
  }
}
