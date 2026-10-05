import fs from "node:fs";
import * as toml from "smol-toml";

interface UvLockPackage {
  name?: string;
  version?: string;
}

interface UvLock {
  package?: UvLockPackage[];
}

export function getUvVersionFromUvLock(filePath: string): string | undefined {
  const fileContent = fs.readFileSync(filePath, "utf-8");
  return getUvVersionFromUvLockContent(fileContent);
}

export function getUvVersionFromUvLockContent(
  fileContent: string,
): string | undefined {
  const parsed = toml.parse(fileContent) as UvLock;
  const uvPackage = parsed.package?.find((pkg) => pkg.name === "uv");
  return uvPackage?.version;
}
