import { createRequire } from "node:module";

const req = createRequire(import.meta.url);
for (
  const [label, action] of [
    ["missing local", () => req("./fixtures/does-not-exist.cjs")],
    ["missing package", () => req("parity-definitely-missing")],
    ["missing export", () => req("parity-exports/private.cjs")],
  ] as const
) {
  try {
    action();
    console.log(label, "no throw");
  } catch (error) {
    const value = error as any;
    console.log(
      label,
      value.name,
      value.code,
      Array.isArray(value.requireStack),
      value.requireStack?.length ?? -1,
      typeof value.path,
      typeof value.request,
    );
  }
}
