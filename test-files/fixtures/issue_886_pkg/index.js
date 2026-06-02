// V8-fallback JS module used by `test_issue_886_jsruntime_http_link.ts`
// to force `jsruntime_lib = Some(...)` in the link path. The module
// itself is irrelevant — its mere presence triggers the
// `Generated JS bundle` path and adds libperry_jsruntime.a to the link
// command, which (pre-fix) caused the `else if ctx.needs_stdlib` branch
// in `link.rs::build_and_run_link` to be skipped entirely and the
// well-known libs to silently drop off the link line.
//
// Keep the surface minimal so the JS-bundle stage stays cheap.
module.exports = {
  fallback_marker: "issue-886-jsruntime-presence",
};
