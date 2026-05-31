import path from "node:path";

const cases = [
  ["relative null from", () => path.relative(null as any, "/x")],
  ["relative null to", () => path.relative("/x", null as any)],
  ["posix relative number", () => path.posix.relative(1 as any, "/x")],
  ["win32 relative object", () => path.win32.relative("C:\\x", {} as any)],
] as const;

for (const [label, fn] of cases) {
  try {
    console.log(label + ":", fn());
  } catch (err: any) {
    console.log(
      label + ":",
      err?.name,
      err?.code || "no-code",
      /string/.test(String(err?.message)),
    );
  }
}
