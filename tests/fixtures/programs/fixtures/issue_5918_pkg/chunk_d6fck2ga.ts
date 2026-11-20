// Mirrors remeda's dist/chunk-D6FCK2GA.js shape (a `purry`-style curry
// helper). Exported under the short, minifier-style name "a" on purpose —
// the bug this fixture reproduces only fires when multiple chunks in the
// same import graph happen to export under colliding short names.
function u(o: (...args: any[]) => any, n: any[], a: unknown) {
  const t = (r: any) => o(r, ...n)
  return a === void 0 ? t : Object.assign(t, { lazy: a, lazyArgs: n })
}
export { u as a }
