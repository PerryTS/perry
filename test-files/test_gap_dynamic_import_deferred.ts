// Issue #753 — a module reached only through dynamic `import()` must
// have its top-level side effects suppressed until the dispatch fires.
// `dynamic_import_deferred_marker.ts` logs "deferred-init-ran" at the
// top level; with the eager/deferred split, that line must appear AFTER
// "before" and BEFORE the import's resolved value is consumed. Pre-#753
// the marker fired at program start (between "" and "before"), proving
// the heavy import path was paid eagerly.

console.log("before");
const branch: number = Number(process.argv[2] ?? "0");
if (branch === 1) {
  const m = await import("./dynamic_import_deferred_marker.ts");
  console.log("after:" + m.x);
} else {
  console.log("skipped");
}
