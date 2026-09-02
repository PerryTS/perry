// #9413, CommonJS arm: the same three leaks, in a module goal where the
// compiler additionally runs a source-level CJS wrap. `.ts` in this repo is
// ESM (`"type": "module"`), so this file is the only place the CJS lowering
// path is exercised.
//
class Named {}
function scopeA() { class Made { } return Made.name; }
class Made {}
const LocalAnon = class {};

console.log("decl:", Named.name);
console.log("ctor:", new Named().constructor.name);
console.log("shadowed:", Made.name, scopeA());
console.log("new-anon:", new (class {})().constructor.name);
console.log("new-named:", new (class Zed {})().constructor.name);
console.log("String:", String(Named));
console.log("inspect:", Named);
console.log("local-anon:", LocalAnon.name);

// #9468: a member assignment is not a NamedEvaluation context. The CJS
// pre-parse rewrite may give this class a synthetic registration key, but that
// key must not become the constructor's observable name.
module.exports = class {};
console.log("module-exports-anon:", module.exports.name);
