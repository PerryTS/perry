// #13558 — a module's top-level `this` is `undefined` (ECMA-262 §16.2.1.6.4:
// a Module Environment Record's `[[ThisValue]]` starts as undefined).
//
// `test-files/` sits under the repo's `"type": "module"` package.json, so this
// file is an ES module for both Perry and the `node --experimental-strip-types`
// oracle without needing a single `import`/`export` — the format alone decides
// the module goal. Pre-fix Perry lowered the top-level `this` to its CommonJS
// `module.exports` stand-in and printed `false` where Node prints `true`
// (test262 language/module-code/eval-this.js).
console.log("this===undefined:", this === undefined);
console.log("typeof this:", typeof this);
// `this` is lexical for an arrow and inherited by direct eval, so both observe
// the same module-level `undefined`.
console.log("arrow:", (() => this)() === undefined);
console.log("eval:", eval("this") === undefined);
// Module code is strict, so a property write on that `undefined` is a
// TypeError, not a silent no-op.
try {
  (this as any).marker = 1;
  console.log("write: no throw");
} catch (e) {
  console.log("write: TypeError", e instanceof TypeError);
}
