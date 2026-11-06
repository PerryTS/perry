// Mirrors remeda's dist/chunk-ANXBDSUI.js shape (shared "done"/"hasNext"
// iterator-result singletons + constructors). Exports THREE bindings under
// short names "a"/"b"/"c" — "c" is the one a sibling chunk's local alias
// "a" will point at, which is what the colliding-key bug clobbers.
const e = { done: true, hasNext: false }
const s = { done: false, hasNext: false }
const a = () => e
const o = (t: unknown) => ({ hasNext: true, next: t, done: false })
export { s as a, a as b, o as c }
