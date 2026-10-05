import type { ResolutionStrategy } from "../utils/inputs";

export type VersionSource =
  | "input"
  | "version-file"
  | "uv.toml"
  | "pyproject.toml"
  | "default";

export type VersionFileFormat =
  | ".tool-versions"
  | "pyproject.toml"
  | "requirements"
  | "uv.lock"
  | "uv.toml";

export interface ParsedVersionFile {
  format: VersionFileFormat;
  specifier: string;
}

export interface ResolveUvVersionOptions {
  manifestFile?: string;
  resolutionStrategy?: ResolutionStrategy;
  version?: string;
  versionFile?: string;
  workingDirectory: string;
}

export interface VersionRequest {
  format?: VersionFileFormat;
  source: VersionSource;
  sourcePath?: string;
  specifier: string;
}
