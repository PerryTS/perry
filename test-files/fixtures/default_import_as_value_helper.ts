// Helper module for test_default_import_as_value.ts. The shape
// `function NAME(...) {…}; export default NAME` is the canonical npm
// barrel idiom — every file under `node_modules/ramda/es/*.js`,
// `node_modules/date-fns/*.js`, etc. uses it.
function add(a: number, b: number): number {
  return a + b;
}
export default add;
