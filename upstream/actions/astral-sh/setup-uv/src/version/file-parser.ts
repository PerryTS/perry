import fs from "node:fs";
import { getConfigValueFromTomlContent } from "../utils/config-file";
import * as log from "../utils/logging";
import {
  getUvVersionFromParsedPyproject,
  getUvVersionFromRequirementsText,
  parsePyprojectContent,
} from "./requirements-file";
import { normalizeVersionSpecifier } from "./specifier";
import { getUvVersionFromToolVersions } from "./tool-versions-file";
import type { ParsedVersionFile, VersionFileFormat } from "./types";
import { getUvVersionFromUvLock } from "./uv-lock-file";

interface VersionFileParser {
  format: VersionFileFormat;
  parse(filePath: string): string | undefined;
  supports(filePath: string): boolean;
}

const VERSION_FILE_PARSERS: VersionFileParser[] = [
  {
    format: ".tool-versions",
    parse: (filePath) => getUvVersionFromToolVersions(filePath),
    supports: (filePath) => filePath.endsWith(".tool-versions"),
  },
  {
    format: "uv.toml",
    parse: (filePath) => {
      const fileContent = fs.readFileSync(filePath, "utf-8");
      return getConfigValueFromTomlContent(
        filePath,
        fileContent,
        "required-version",
      );
    },
    supports: (filePath) => filePath.endsWith("uv.toml"),
  },
  {
    format: "pyproject.toml",
    parse: (filePath) => {
      const fileContent = fs.readFileSync(filePath, "utf-8");
      const pyproject = parsePyprojectContent(fileContent);
      const requiredVersion = pyproject.tool?.uv?.["required-version"];

      if (requiredVersion !== undefined) {
        return requiredVersion;
      }

      return getUvVersionFromParsedPyproject(pyproject);
    },
    supports: (filePath) => filePath.endsWith("pyproject.toml"),
  },
  {
    format: "uv.lock",
    parse: (filePath) => getUvVersionFromUvLock(filePath),
    supports: (filePath) => filePath.endsWith("uv.lock"),
  },
  {
    format: "requirements",
    parse: (filePath) => {
      const fileContent = fs.readFileSync(filePath, "utf-8");
      return getUvVersionFromRequirementsText(fileContent);
    },
    supports: (filePath) => filePath.endsWith(".txt"),
  },
];

export function getParsedVersionFile(
  filePath: string,
): ParsedVersionFile | undefined {
  log.info(`Trying to find version for uv in: ${filePath}`);

  if (!fs.existsSync(filePath)) {
    log.info(`Could not find file: ${filePath}`);
    return undefined;
  }

  const parser = getVersionFileParser(filePath);
  if (parser === undefined) {
    return undefined;
  }

  try {
    const specifier = parser.parse(filePath);
    if (specifier === undefined) {
      return undefined;
    }

    const normalizedSpecifier = normalizeVersionSpecifier(specifier);
    log.info(`Found version for uv in ${filePath}: ${normalizedSpecifier}`);
    return {
      format: parser.format,
      specifier: normalizedSpecifier,
    };
  } catch (error) {
    log.warning(`Error while parsing ${filePath}: ${(error as Error).message}`);
    return undefined;
  }
}

export function getUvVersionFromFile(filePath: string): string | undefined {
  return getParsedVersionFile(filePath)?.specifier;
}

function getVersionFileParser(filePath: string): VersionFileParser | undefined {
  return VERSION_FILE_PARSERS.find((parser) => parser.supports(filePath));
}
