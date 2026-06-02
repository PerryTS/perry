// Regression test for #444: `import.meta.url` returned `0` and
// `import.meta.main` returned `NaN` because the bare `import.meta`
// expression was lowered to an `Expr::Object` literal at module top
// level, which trips the long-standing module-globals NaN-boxing bug
// where string fields read back as 0 (see auto-memory's
// project_hone_crash). The user-visible symptom: `if (import.meta.main)
// main()` never fired and `new URL("./x", import.meta.url)` got
// nonsense.
//
// Fix folds `import.meta.<prop>` directly to a string/bool literal at
// the Member expression lowering site, bypassing the broken Object
// path entirely. The bare-`import.meta` Object synthesis stays as a
// fallback for the rare cases that use it as a value (spread,
// destructure, etc.).
//
// Surface aligned with Node 20+ spec: `url`, `main`, `dirname`,
// `filename`. Bun-only aliases (`dir` / `path` / `file`) intentionally
// not exposed — would silently break code moving Perry → Node.

console.log("url is string:", typeof import.meta.url === "string");
console.log("url starts with file://:", import.meta.url.startsWith("file://"));

console.log("main is boolean:", typeof import.meta.main === "boolean");
console.log("main is true:", import.meta.main === true);

console.log("dirname is string:", typeof import.meta.dirname === "string");
console.log("filename is string:", typeof import.meta.filename === "string");

// Don't pin absolute paths — those vary by runner CWD — pin shapes.
console.log(
  "filename ends with .ts:",
  import.meta.filename.endsWith(".ts"),
);
console.log(
  "url contains filename suffix:",
  import.meta.url.endsWith("test_issue_444_import_meta_props.ts"),
);

// The canonical entry-guard pattern from the issue body.
if (import.meta.main) {
  console.log("entry guard fired");
}
