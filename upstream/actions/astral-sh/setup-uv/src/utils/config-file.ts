import fs from "node:fs";
import * as toml from "smol-toml";

export function getConfigValueFromTomlFile(
  filePath: string,
  key: string,
): string | undefined {
  if (!fs.existsSync(filePath) || !filePath.endsWith(".toml")) {
    return undefined;
  }

  const fileContent = fs.readFileSync(filePath, "utf-8");
  return getConfigValueFromTomlContent(filePath, fileContent, key);
}

export function getConfigValueFromTomlContent(
  filePath: string,
  fileContent: string,
  key: string,
): string | undefined {
  if (!filePath.endsWith(".toml")) {
    return undefined;
  }

  if (filePath.endsWith("pyproject.toml")) {
    const tomlContent = toml.parse(fileContent) as {
      tool?: { uv?: Record<string, string | undefined> };
    };
    return tomlContent?.tool?.uv?.[key];
  }

  const tomlContent = toml.parse(fileContent) as Record<
    string,
    string | undefined
  >;
  return tomlContent[key];
}
