import { findPackageJSON } from "node:module";

const normalize = (value: unknown) =>
  String(value).replace(process.cwd(), "<cwd>");
for (
  const [specifier, base] of [
    [
      "./require/fixtures/find-package/nested/probe.cjs",
      new URL("../entry.ts", import.meta.url).href,
    ],
    ["parity-exports", new URL("../require/entry.ts", import.meta.url).href],
    ["node:fs", import.meta.url],
  ] as const
) {
  try {
    console.log(specifier, normalize(findPackageJSON(specifier, base)));
  } catch (error) {
    console.log(
      specifier,
      (error as any).name,
      (error as any).code ?? "no-code",
    );
  }
}
for (const value of [undefined, null, 1, {}, ""] as any[]) {
  try {
    console.log(
      "bad",
      Object.prototype.toString.call(value),
      normalize(findPackageJSON(value, import.meta.url)),
    );
  } catch (error) {
    console.log(
      "bad",
      Object.prototype.toString.call(value),
      (error as any).name,
      (error as any).code ?? "no-code",
    );
  }
}
