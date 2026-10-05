import * as path from "node:path";
import * as log from "../utils/logging";
import { getParsedVersionFile } from "./file-parser";
import { normalizeVersionSpecifier } from "./specifier";
import type {
  ParsedVersionFile,
  ResolveUvVersionOptions,
  VersionRequest,
} from "./types";

export interface VersionRequestResolver {
  resolve(context: VersionRequestContext): VersionRequest | undefined;
}

export class VersionRequestContext {
  readonly version: string | undefined;
  readonly versionFile: string | undefined;
  readonly workingDirectory: string;

  private readonly parsedFiles = new Map<
    string,
    ParsedVersionFile | undefined
  >();

  constructor(
    version: string | undefined,
    versionFile: string | undefined,
    workingDirectory: string,
  ) {
    this.version = version;
    this.versionFile = versionFile;
    this.workingDirectory = workingDirectory;
  }

  getVersionFile(filePath: string): ParsedVersionFile | undefined {
    const cachedResult = this.parsedFiles.get(filePath);
    if (cachedResult !== undefined || this.parsedFiles.has(filePath)) {
      return cachedResult;
    }

    const result = getParsedVersionFile(filePath);
    this.parsedFiles.set(filePath, result);
    return result;
  }

  getWorkspaceCandidates(): Array<{
    source: "pyproject.toml" | "uv.toml";
    sourcePath: string;
  }> {
    return [
      {
        source: "uv.toml",
        sourcePath: path.join(this.workingDirectory, "uv.toml"),
      },
      {
        source: "pyproject.toml",
        sourcePath: path.join(this.workingDirectory, "pyproject.toml"),
      },
    ];
  }
}

export class ExplicitInputVersionResolver implements VersionRequestResolver {
  resolve(context: VersionRequestContext): VersionRequest | undefined {
    if (context.version === undefined) {
      return undefined;
    }

    return {
      source: "input",
      specifier: normalizeVersionSpecifier(context.version),
    };
  }
}

export class VersionFileVersionResolver implements VersionRequestResolver {
  resolve(context: VersionRequestContext): VersionRequest | undefined {
    if (context.versionFile === undefined) {
      return undefined;
    }

    const versionFile = context.getVersionFile(context.versionFile);
    if (versionFile === undefined) {
      throw new Error(
        `Could not determine uv version from file: ${context.versionFile}`,
      );
    }

    return {
      format: versionFile.format,
      source: "version-file",
      sourcePath: context.versionFile,
      specifier: versionFile.specifier,
    };
  }
}

export class WorkspaceVersionResolver implements VersionRequestResolver {
  resolve(context: VersionRequestContext): VersionRequest | undefined {
    for (const candidate of context.getWorkspaceCandidates()) {
      const versionFile = context.getVersionFile(candidate.sourcePath);
      if (versionFile === undefined) {
        continue;
      }

      return {
        format: versionFile.format,
        source: candidate.source,
        sourcePath: candidate.sourcePath,
        specifier: versionFile.specifier,
      };
    }

    log.info(
      "Could not determine uv version from uv.toml or pyproject.toml. Falling back to latest.",
    );
    return undefined;
  }
}

export class LatestVersionResolver implements VersionRequestResolver {
  resolve(): VersionRequest {
    return {
      source: "default",
      specifier: "latest",
    };
  }
}

const VERSION_REQUEST_RESOLVERS: VersionRequestResolver[] = [
  new ExplicitInputVersionResolver(),
  new VersionFileVersionResolver(),
  new WorkspaceVersionResolver(),
  new LatestVersionResolver(),
];

export function resolveVersionRequest(
  options: ResolveUvVersionOptions,
): VersionRequest {
  const context = new VersionRequestContext(
    emptyToUndefined(options.version),
    emptyToUndefined(options.versionFile),
    options.workingDirectory,
  );

  for (const resolver of VERSION_REQUEST_RESOLVERS) {
    const request = resolver.resolve(context);
    if (request !== undefined) {
      return request;
    }
  }

  throw new Error("Could not resolve a requested uv version.");
}

function emptyToUndefined(value: string | undefined): string | undefined {
  return value === undefined || value === "" ? undefined : value;
}
