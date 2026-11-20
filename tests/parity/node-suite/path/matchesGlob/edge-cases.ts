import path from "node:path";

function show(p: string, pattern: string): void {
  try { console.log(p + " ~ " + pattern + ":", path.matchesGlob(p, pattern)); } catch (err: any) { console.log(p + " ~ " + pattern + ":", err?.name, err?.code || "no-code"); }
}
show("src/app.ts", "src/*.ts");
show("src/nested/app.ts", "src/*.ts");
show("src/nested/app.ts", "src/**/*.ts");
show("README.md", "*.{md,txt}");
show("a/b", "a/**");
