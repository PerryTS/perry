import { createRequire } from "node:module";
import { fileURLToPath, pathToFileURL } from "node:url";

const filename = fileURLToPath(import.meta.url);
const slash = (value: string) => value.replaceAll("\\", "/");
const bases = [
  ["url object", pathToFileURL(filename)],
  ["url string", pathToFileURL(filename).href],
  ["absolute", filename],
] as const;
for (const [label, base] of bases) {
  const req = createRequire(base);
  console.log(
    label,
    slash(req.resolve("./fixtures/value.cjs")).endsWith("/fixtures/value.cjs"),
  );
}

for (
  const value of ["./relative.js", "https://example.test/a.js", {}, null, 1]
) {
  try {
    createRequire(value as any);
    console.log(Object.prototype.toString.call(value), "no throw");
  } catch (error) {
    console.log(
      Object.prototype.toString.call(value),
      (error as any).name,
      (error as any).code ?? "no-code",
    );
  }
}
