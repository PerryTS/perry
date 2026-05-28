// #2076: function .name inference for named function expressions and
// object-literal shorthand methods. Verified against
// `node --experimental-strip-types`.

// (1) Named function expression — own name wins over binding name.
const bar = function namedBar() {};
console.log(bar.name);

// (2) Object-literal shorthand method — key becomes the function name.
const obj = { method() {} };
console.log(obj.method.name);

// (3) Console formatting picks up the same registry.
console.log(bar);
console.log(obj.method);

// (4) Regression: anonymous arrow assigned to a `const` still inherits
// the binding name (existing behavior must not break).
const baz = () => {};
console.log(baz.name);

// (5) Regression: named declaration `function foo(){}` still resolves
// to its own name.
function foo() {}
console.log(foo.name);
console.log(foo);
