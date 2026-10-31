// Gap test: a function declaration's binding is mutable, and calls read it
// Run: node --experimental-strip-types test_gap_function_declaration_reassign.ts

// --- A declaration that reassigns itself, then calls itself (Babel's _typeof) ---
function _typeof(o: any): any {
  return (
    (_typeof =
      "function" == typeof Symbol && "symbol" == typeof Symbol.iterator
        ? function (o$1: any) {
            return typeof o$1;
          }
        : function (o$1: any) {
            return o$1 &&
              "function" == typeof Symbol &&
              o$1.constructor === Symbol &&
              o$1 !== Symbol.prototype
              ? "symbol"
              : typeof o$1;
          }),
    _typeof(o)
  );
}
console.log(_typeof("a"), _typeof(1), _typeof(null), _typeof(Symbol("x")));
console.log(_typeof(undefined), _typeof({}), _typeof(() => 1));

// --- Module-level reassignment, observed by a direct call and a caller ---
function greet(): string {
  return "original";
}
function callGreet(): string {
  return greet();
}
console.log(greet(), callGreet());
greet = function (): string {
  return "replaced";
};
console.log(greet(), callGreet());

// --- Reassignment from a closure ---
function ping(): string {
  return "ping";
}
const swapPing = () => {
  ping = () => "pong";
};
console.log(ping());
swapPing();
console.log(ping());

// --- A nested declaration that reassigns itself ---
function outer(): string {
  function inner(n: number): string {
    inner = function (m: number): string {
      return "second:" + m;
    };
    return "first:" + n;
  }
  return [inner(1), inner(2), inner(3)].join(",");
}
console.log(outer());
console.log(outer());

// --- Shadowing names never touch the declaration ---
function keep(): string {
  return "kept";
}
function shadowedByParam(keep: any): any {
  keep = "param";
  return keep;
}
function shadowedByLocal(): any {
  let keep: any = 0;
  keep = "local";
  return keep;
}
console.log(shadowedByParam("x"), shadowedByLocal(), keep());
