// #9413, CommonJS arm: the same three leaks, in a module goal where the
// compiler additionally runs a source-level CJS wrap. `.ts` in this repo is
// ESM (`"type": "module"`), so this file is the only place the CJS lowering
// path is exercised.
//
// Deliberately NOT covered here: `module.exports = class {}` and
// `exports.Foo = class {}`. Both are member assignments, which per spec get no
// NamedEvaluation (node: `""`), but perry's CJS source rewrite turns the first
// into a NAMED declaration (`__perry_cjs_default__`) before parsing, and drops
// the binding-name inference for the local-`const` form. Those are defects of
// `crates/perry/src/commands/compile/cjs_wrap/hoist_classes.rs`, upstream of
// anything class metadata can reach — reported separately.
class Named {}
function scopeA() { class Made { } return Made.name; }
class Made {}

console.log("decl:", Named.name);
console.log("ctor:", new Named().constructor.name);
console.log("shadowed:", Made.name, scopeA());
console.log("new-anon:", new (class {})().constructor.name);
console.log("new-named:", new (class Zed {})().constructor.name);
console.log("String:", String(Named));
console.log("inspect:", Named);
