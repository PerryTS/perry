// Minimal fixture for tier 12 link_smoke. Verifies the runtime + stdlib
// cross-compile + link pipeline works for every target Perry advertises.
//
// Intentionally console-only (no `import "perry/ui"`): UI staticlibs need
// to be pre-built per target via `cargo build --release -p perry-ui-<t>
// --target <triple>`, and that's a tier-0 / tier-1 prerequisite, not a
// smoke-test responsibility. Per-platform UI link paths are exercised by
// tiers 7 (host), 8/9 (Apple sims), 10 (Android emu), 11 (Windows).
//
// What this fixture DOES exercise per target: NaN-box runtime symbols,
// stdlib resolution, format!/println paths through Perry's console.log,
// the auto-optimized stdlib rebuild for each cross-target triple.

console.log("ok");
