import { createRequire } from "node:module";

const req = createRequire(import.meta.url);
const slash = (value: string) => value.replaceAll("\\", "/");
const normalize = (value: string) =>
  slash(value.replace(process.cwd(), "<cwd>"));
for (
  const specifier of [
    "fs",
    "node:fs",
    "./fixtures/value",
    "./fixtures/value.cjs",
    "./fixtures/pkg-main",
    "parity-exports",
    "parity-exports/feature",
  ]
) {
  try {
    console.log("resolve", specifier, normalize(req.resolve(specifier)));
  } catch (error) {
    console.log(
      "resolve",
      specifier,
      (error as any).name,
      (error as any).code ?? "no-code",
    );
  }
}
for (
  const specifier of [
    "fs",
    "node:fs",
    "./fixtures/value.cjs",
    "parity-exports",
    "missing-parity-package",
  ]
) {
  const paths = req.resolve.paths(specifier);
  console.log(
    "paths",
    specifier,
    paths === null
      ? "null"
      : `${Array.isArray(paths)}:${
        paths!.some((path) => slash(path).endsWith("/require/node_modules"))
      }`,
  );
}
