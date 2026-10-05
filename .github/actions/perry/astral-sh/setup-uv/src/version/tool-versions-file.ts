import fs from "node:fs";
import * as core from "@actions/core";

export function getUvVersionFromToolVersions(
  filePath: string,
): string | undefined {
  const versions = getToolVersions(filePath, "uv");
  if (versions === undefined || versions.length !== 1) {
    return undefined;
  }

  const version = stripVersionPrefix(versions[0]);
  if (isPath(version)) {
    core.warning(
      `The uv version ${versions[0]} in .tool-versions is not supported. Paths are not allowed.`,
    );
    return undefined;
  }
  if (version.startsWith("ref")) {
    core.warning(
      "The ref syntax of .tool-versions is not supported. Please use a released version instead.",
    );
    return undefined;
  }
  return version;
}

export function getPythonVersionFromToolVersions(
  filePath: string,
): string | undefined {
  const versions = getToolVersions(filePath, "python");
  if (versions === undefined || versions.length === 0) {
    return undefined;
  }
  if (versions.length > 1) {
    core.warning(
      "Multiple Python versions in .tool-versions are not supported. The Python entry will be ignored.",
    );
    return undefined;
  }

  const version = stripVersionPrefix(versions[0]);
  if (
    version === "system" ||
    version.startsWith("ref:") ||
    version.startsWith("path:") ||
    isPath(version)
  ) {
    core.warning(
      `The Python version ${versions[0]} in .tool-versions is not supported. The Python entry will be ignored.`,
    );
    return undefined;
  }
  return version;
}

function getToolVersions(
  filePath: string,
  toolName: string,
): string[] | undefined {
  if (!filePath.endsWith(".tool-versions")) {
    return undefined;
  }
  const fileContents = fs.readFileSync(filePath, "utf8");

  for (const line of fileContents.split("\n")) {
    const content = line.split("#", 1)[0].trim();
    if (content === "") {
      continue;
    }

    const [tool, ...versions] = content.split(/\s+/);
    if (tool === toolName) {
      return versions;
    }
  }
  return undefined;
}

function stripVersionPrefix(version: string): string {
  return version.startsWith("v") ? version.slice(1) : version;
}

function isPath(version: string): boolean {
  return version.includes("/") || version.includes("\\");
}
