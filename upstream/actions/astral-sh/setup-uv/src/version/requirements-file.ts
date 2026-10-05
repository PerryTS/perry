import fs from "node:fs";
import * as toml from "smol-toml";

export function getUvVersionFromRequirementsFile(
  filePath: string,
): string | undefined {
  const fileContent = fs.readFileSync(filePath, "utf-8");

  if (filePath.endsWith(".txt")) {
    return getUvVersionFromRequirementsText(fileContent);
  }

  return getUvVersionFromPyprojectContent(fileContent);
}

export function getUvVersionFromRequirementsText(
  fileContent: string,
): string | undefined {
  return getUvVersionFromAllDependencies(fileContent.split("\n"));
}

export function getUvVersionFromParsedPyproject(
  pyproject: Pyproject,
): string | undefined {
  const dependencies: string[] = pyproject?.project?.dependencies || [];
  const optionalDependencies: string[] = Object.values(
    pyproject?.project?.["optional-dependencies"] || {},
  ).flat();
  const devDependencies: string[] = Object.values(
    pyproject?.["dependency-groups"] || {},
  )
    .flat()
    .filter((item: string | object) => typeof item === "string");

  return getUvVersionFromAllDependencies(
    dependencies.concat(optionalDependencies, devDependencies),
  );
}

export function getUvVersionFromPyprojectContent(
  pyprojectContent: string,
): string | undefined {
  const pyproject = parsePyprojectContent(pyprojectContent);
  return getUvVersionFromParsedPyproject(pyproject);
}

export interface Pyproject {
  project?: {
    dependencies?: string[];
    "optional-dependencies"?: Record<string, string[]>;
  };
  "dependency-groups"?: Record<string, Array<string | object>>;
  tool?: {
    uv?: Record<string, string | undefined>;
  };
}

export function parsePyprojectContent(pyprojectContent: string): Pyproject {
  return toml.parse(pyprojectContent) as Pyproject;
}

function getUvVersionFromAllDependencies(
  allDependencies: string[],
): string | undefined {
  return allDependencies
    .map(getUvVersionFromDependency)
    .find((version): version is string => version !== undefined);
}

function getUvVersionFromDependency(dependency: string): string | undefined {
  const dependencyWithoutMarker = dependency.split(";", 1)[0]?.trim();
  return dependencyWithoutMarker?.match(/^uv([=<>~!]+\S*)/)?.[1].trim();
}
