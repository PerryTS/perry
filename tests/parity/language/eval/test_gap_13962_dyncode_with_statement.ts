// #13962: `with` inside runtime-built `new Function` bodies. The bodies are
// joined at run time so the interpreter, not the constant folder, runs them.
const sep = process.argv.length > 99 ? "x" : "\n";
const make = (params: string[], lines: string[]) => new Function(...params, lines.join(sep));

// Lookup through the object, fallthrough to an outer binding and a global.
const lookup = make(["obj"], [
  "var outer = 'outer';",
  "with (obj) {",
  "  return [a + 1, outer, typeof Math.max, typeof missing].join(',');",
  "}",
]);
console.log(lookup({ a: 41 }));

// Assignment to an existing property writes the object; to an absent name
// writes the enclosing `var`.
const assign = make(["obj"], [
  "var local = 0;",
  "with (obj) {",
  "  a = a * 2;",
  "  local = 5;",
  "  var b = 'via-var';",
  "}",
  "return [obj.a, local, obj.b, b, 'local' in obj].join(',');",
]);
console.log(assign({ a: 21, b: 'own' }));
console.log(assign({ a: 1 }));

// Properties inherited through the prototype chain are bindings too.
const inherited = make(["obj"], ["with (obj) { return greeting + '!'; }"]);
console.log(inherited(Object.create({ greeting: "hi" })));

// Symbol.unscopables hides a binding from the with-environment.
const unscopable = make(["obj"], [
  "var hidden = 'outer';",
  "with (obj) { return hidden + ',' + shown; }",
]);
console.log(unscopable({ hidden: "inner", shown: "s", [Symbol.unscopables]: { hidden: true } }));
// Arrays are with-objects too: own `length`, inherited methods.
const arrayScope = make(["arr"], ["with (arr) { return [length, typeof push, join('-')].join(','); }"]);
console.log(arrayScope([1, 2, 3]));

// A function found through the with-object is called with it as `this`.
const methodThis = make(["obj"], ["with (obj) { return describe(); }"]);
console.log(methodThis({ name: "box", describe() { return "I am " + this.name; } }));

// Primitives are boxed; null and undefined throw a TypeError.
const prim = make(["v"], ["with (v) { return length + ':' + charAt(0); }"]);
console.log(prim("hello"));
for (const bad of [null, undefined]) {
  try {
    make(["v"], ["with (v) { return 1; }"])(bad);
  } catch (e) {
    console.log((e as Error).constructor.name, (e as Error).message);
  }
}

// Closures created inside `with` keep the object environment.
const closure = make(["obj"], ["with (obj) { return function () { return count++; }; }"]);
const state = { count: 10 };
const next = closure(state) as () => number;
next();
next();
console.log(state.count);

// The shape of lodash's `_.template` compiled source (no `variable` option).
const tmpl = make(["obj"], [
  "obj || (obj = {});",
  "var __t, __p = '';",
  "with (obj) {",
  "__p += 'hello ' +",
  "((__t = ( user )) == null ? '' : __t) +",
  "'!';",
  "}",
  "return __p",
]);
console.log(tmpl({ user: "fred" }));
console.log(tmpl({ user: null }));
