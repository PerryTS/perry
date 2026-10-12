class C { ["\uD800"]() { return 1; } ["\uDC00"]() { return 2; } [""]() { return 3; } static ["\uD800"]() { return 4; } }
const c = new C();
console.log("methods", c["\uD800"](), c["\uDC00"](), c[""](), C["\uD800"]());
console.log("names", c["\uD800"].name.length, c["\uD800"].name.charCodeAt(0), c["\uDC00"].name.charCodeAt(0));
const D = class { ["\uD800"]() { return 5; } };
console.log("expression", new D()["\uD800"]());
