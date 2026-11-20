// Coverage probe for issue #1069's "arguments magic object" sub-target.
// Walks through every pattern Effect's `dual()` plus general Node idioms
// rely on. Each block prints something Node prints byte-for-byte, so we
// can diff against `node --experimental-strip-types`.

// 1. Basic arguments.length and indexed access.
function args1(a: any, b: any) {
  return [arguments.length, arguments[0], arguments[1], arguments[2]];
}
console.log("1a:", JSON.stringify(args1()));                  // [0,undefined,undefined,undefined]
console.log("1b:", JSON.stringify(args1(10)));                // [1,10,undefined,undefined]
console.log("1c:", JSON.stringify(args1(10, 20)));            // [2,10,20,undefined]
console.log("1d:", JSON.stringify(args1(10, 20, 30, 40)));    // [4,10,20,30]

// 2. Spread of arguments into another call.
function args2() {
  return Math.max(...arguments as any);
}
console.log("2:", args2(3, 1, 4, 1, 5, 9, 2, 6));             // 9

// 3. Array.from(arguments).
function args3() {
  return Array.from(arguments).join("-");
}
console.log("3:", args3(1, 2, 3, "x"));                       // 1-2-3-x

// 4. arguments forwarded into .apply(this, arguments).
function inner4(this: any, x: number, y: number, z: number) {
  return x * 100 + y * 10 + z;
}
function outer4() {
  return (inner4 as any).apply(null, arguments);
}
console.log("4:", outer4(1, 2, 3));                           // 123

// 5. for-loop over arguments by index.
function args5() {
  let s = "";
  for (let i = 0; i < arguments.length; i++) s += arguments[i];
  return s;
}
console.log("5:", args5("a", "b", "c", "d"));                 // abcd

// 6. arguments inside a nested arrow inherits the enclosing function's.
function args6() {
  const inner = () => arguments[0];
  return inner();
}
console.log("6:", args6("captured"));                         // captured

// 7. arguments.length in a returned FnExpr (the gap-1 pattern).
function make7() {
  return function (a: any, b: any) {
    return arguments.length;
  };
}
const f7 = make7();
console.log("7a:", f7());                                     // 0
console.log("7b:", f7(1, 2, 3));                              // 3

// 8. Effect-style dual() dispatcher.
function dual8(arity: number, body: any): any {
  return function () {
    if (arguments.length >= arity) {
      if (arguments.length === 2) return body(arguments[0], arguments[1]);
      if (arguments.length === 3) return body(arguments[0], arguments[1], arguments[2]);
      return body();
    }
    const first = arguments[0];
    return function (self: any) { return body(self, first); };
  };
}
const hasProperty8 = dual8(2, (self: any, prop: any) =>
  typeof self === "object" && self !== null && prop in self);
console.log("8a:", hasProperty8({ k: 1 }, "k"));              // true
console.log("8b:", hasProperty8({ k: 1 }, "x"));              // false
console.log("8c:", typeof hasProperty8("k"));                 // function
console.log("8d:", (hasProperty8("k") as any)({ k: 1 }));     // true

// 9. Class method reading arguments.
class C9 {
  m() {
    return arguments.length;
  }
  static s() {
    return arguments.length;
  }
}
const c9 = new C9();
console.log("9a:", c9.m(1, 2, 3));                            // 3
console.log("9b:", C9.s("a", "b", "c", "d", "e"));            // 5
