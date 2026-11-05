// Repro for gap 1 from PR #899: arguments.length in returned FnExpr with fixed params.
//
// Effect uses `dual(arity, body)` to dispatch data-first vs data-last based on
// arguments.length at the call site. The body is a `function (a, b) { ... }`
// returned from a factory; perry's synthesized `...arguments` rest-param only
// captures TRAILING args after the fixed params, so f(1, 2) was reporting
// `arguments.length === 0`.
//
// The fix should make `function (a, b) { return arguments.length }` see all
// passed args, not just trailing.

function makeFn() {
  return function (a: any, b: any) {
    return arguments.length;
  };
}

const f = makeFn();
console.log(f());            // expect: 0
console.log(f(10));          // expect: 1
console.log(f(10, 20));      // expect: 2
console.log(f(10, 20, 30));  // expect: 3

// Direct (non-returned) function expression assigned to a const must keep
// working too.
const g = function (a: any, b: any) {
  return arguments.length;
};
console.log(g(1));           // expect: 1
console.log(g(1, 2, 3, 4));  // expect: 4

// Effect-style dispatcher returned from a factory: the returned function
// reads `arguments.length` to discriminate data-first vs data-last.
function makeDispatcher() {
  return function (a: any, b: any) {
    return arguments.length === 2 ? "data-first" : "curried";
  };
}

const dispatch = makeDispatcher();
console.log(dispatch(1, 2));   // expect: data-first
console.log(dispatch(1));      // expect: curried
