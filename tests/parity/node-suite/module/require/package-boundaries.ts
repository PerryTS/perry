import { createRequire } from "node:module";

const req = createRequire(import.meta.url);
const slash = (value: string) => value.replaceAll("\\", "/");
console.log(
  "main:",
  req("./fixtures/pkg-main").kind,
  slash(req.resolve("./fixtures/pkg-main")).endsWith("/pkg-main/lib/entry.cjs"),
);
const root = req("parity-exports");
console.log("exports root:", root.entry, root.internal);
console.log("exports feature:", req("parity-exports/feature").feature);
for (
  const specifier of [
    "parity-exports/legacy.cjs",
    "parity-exports/package.json",
    "parity-exports/missing",
  ]
) {
  try {
    req(specifier);
    console.log(specifier, "no throw");
  } catch (error) {
    console.log(
      specifier,
      (error as any).name,
      (error as any).code ?? "no-code",
    );
  }
}
