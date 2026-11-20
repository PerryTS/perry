// Issue #851 — vitest's `dist/chunks/test.*.js` is a Rollup-bundled hybrid:
// top-level ESM `import`/`export` statements coexist with inlined CJS bodies
// (`module.exports = factory()`) inside nested helper IIFEs. Pre-fix, Perry's
// `cjs_wrap::is_commonjs` heuristic matched the `module.exports` token and
// wrapped the entire source in a CJS IIFE — which moved the top-level
// `import` *inside* the wrap and made SWC reject the file with
// `ImportExportInScript`. The fix in `cjs_wrap.rs` now treats any file with
// a top-level `import`/`export` statement as ESM and skips the wrap, so the
// CJS-looking tokens inside nested function bodies just lower as ordinary
// identifiers.
//
// This fixture is a minimal ESM-with-CJS-identifiers-inside-a-function shape
// that compiles natively. We don't need the full vitest scenario for the
// parser-mode test — the heuristic is exercised by unit tests in
// `crates/perry/src/commands/compile/cjs_wrap.rs`. This end-to-end test
// guards against a regression where the SWC parser, the HIR lower, or
// later passes would mis-handle bare `module`/`exports`/`require` reads
// inside a function body when the surrounding file is unambiguously ESM.
//
// Note: this fixture verifies the parser path only. A full `vitest`
// compile-package compile may still fail at later pipeline stages (link,
// codegen of obscure CJS shapes); those are tracked separately.
//
// Expected stdout:
//   ok-851

export function fakeRollupHelper(): string {
  // `module` / `exports` / `require` are not in scope here — Perry should
  // simply treat them as unknown identifiers and not attempt to wrap the
  // file as CJS. The pattern below mimics what Rollup emits when it inlines
  // a CJS dep into an ESM bundle (the names are referenced but never
  // executed because `hasRequiredDep` short-circuits the function).
  let hasRequiredDep = true;
  let dep: string = "fallback";
  if (!hasRequiredDep) {
    // unreachable — present only to keep the CJS-shaped tokens around so
    // the parser sees them as references, not just spelling in a string.
    const module: { exports: string } = { exports: "x" };
    const exports = module.exports;
    dep = exports;
  }
  return dep;
}

const v = fakeRollupHelper();
if (v === "fallback") {
  console.log("ok-851");
} else {
  console.log("BAD: v is " + v);
}
