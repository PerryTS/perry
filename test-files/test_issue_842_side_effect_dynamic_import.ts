// Issue #842 — `await import("./X.ts")` against a module with NO
// `export` statements must succeed at link + run time. The helper is
// purely side-effecting (logs once at init). Pre-fix, the producer
// module emitted no `@__perry_ns_<prefix>` global because its
// `namespace_entries` list was empty, while the consumer-side dispatch
// declared the symbol extern unconditionally → undefined-symbol at link.
//
// Expected stdout (deterministic):
//   before-import
//   helper-side-effect-ran
//   after-import
//   ok

console.log("before-import");
const m = await import("./test_issue_842_side_effect_helper.ts");
console.log("after-import");
// The namespace object is observable as `m` even though the source
// module has no exports — it's just an empty object.
if (typeof m === "object") {
  console.log("ok");
} else {
  console.log("BAD: m is " + typeof m);
}
