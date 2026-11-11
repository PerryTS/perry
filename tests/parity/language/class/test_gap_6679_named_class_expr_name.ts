// Issue #6679: a NAMED class EXPRESSION's `.name` must be its own explicit
// class name, not the outer binding name. `const B = class Named {}` binds
// `B`, but the class was declared `Named`, so `B.name === "Named"`. Per spec a
// named class expression is NOT an anonymous function definition, so the
// assignment's NamedEvaluation (`SetFunctionName` from the `const B =` target)
// must not clobber the declared name. perry's module-top-level
// `const X = class {…}` fast path registered the class under the binding name
// and used that for `.name`, so `B.name` wrongly returned "B". An ANONYMOUS
// class expression (`const A = class {}`) still infers "A" — that is correct.
//
// Expected output:
// Named
// A
// Foo
// D
// Named
// Baz
// Meth
// Inner
// Exp
// object

// Module-top-level fast path (the bug site): explicit name wins.
const B = class Named {};
console.log(B.name); // Named

// Control: anonymous class expression correctly infers the binding name.
const A = class {};
console.log(A.name); // A

// Explicit name wins even with static members present.
const C = class Foo {
  static bar() {
    return 1;
  }
};
console.log(C.name); // Foo

// Inner name equal to the binding name: unchanged.
const D = class D {};
console.log(D.name); // D

// An instance's constructor.name follows the class's `.name`.
console.log(new B().constructor.name); // Named

// Assignment name-inference path also honors the explicit name.
let E;
E = class Baz {};
console.log(E.name); // Baz

// Object-property position (general class-expression value arm).
const obj = { m: class Meth {} };
console.log(obj.m.name); // Meth

// Class expression returned from a function body.
function f() {
  return class Inner {};
}
console.log(f().name); // Inner

// Exported named class expression.
export const G = class Exp {};
console.log(G.name); // Exp

// The named class expression is still constructible via the binding.
console.log(typeof new C()); // object
