// Regression test for #6718: spread-calling a native builtin callback method
// (`obj.method(...[fn])`) must SPREAD the argument list, not pass the array
// unspread.
//
// The dense HIR array-method fast paths read the callback from a single
// positional `args[0]`. A spread call collapses the spread operand into that
// slot, so `[1,2].map(...[fn])` handed the spread SOURCE array `[fn]` to the
// callback slot → `TypeError: object is not a function` (and queueMicrotask →
// "callback must be a function"). These callback methods now decline to the
// generic `Expr::CallSpread` tail, which materializes the spread and dispatches
// the native method by name with `this` bound to the receiver — the same path
// `setTimeout(...[fn], 0)` and `p.then(...[fn])` already use.

// --- inline array literals, exactly the reported repro shape --------------
console.log([1, 2].map(...[(x: number) => x * 2]).join(","));            // 2,4
console.log([1, 2, 3].filter(...[(x: number) => x > 1]).join(","));      // 2,3
let sum = 0;
[1, 2, 3].forEach(...[(x: number) => { sum += x; }]);
console.log(sum);                                                        // 6
console.log([1, 2, 3].reduce(...[(a: number, b: number) => a + b, 0]));  // 6
console.log([3, 1, 2].sort(...[(a: number, b: number) => a - b]).join(",")); // 1,2,3
console.log([1, 2, 3].find(...[(x: number) => x === 2]));                // 2
console.log([1, 2, 3].findIndex(...[(x: number) => x === 3]));           // 2
console.log([1, 2, 3, 4].some(...[(x: number) => x > 3]));               // true
console.log([1, 2, 3, 4].every(...[(x: number) => x > 0]));              // true
console.log([[1], [2, 3]].flatMap(...[(x: number[]) => x]).join(","));   // 1,2,3
console.log([1, 2, 3].reduceRight(...[(a: string, b: number) => a + b, "z"])); // z321

// --- local-variable receiver ---------------------------------------------
const arr = [10, 20];
console.log(arr.map(...[(x: number) => x + 1]).join(","));               // 11,21

// --- positional (non-callback) array methods: same root cause ------------
console.log([1, 2, 3, 4].slice(...[1, 3]).join(","));                   // 2,3
console.log([10, 20, 30].indexOf(...[20]));                             // 1
console.log([1, 2, 3].includes(...[2]));                               // true
console.log([1, 2, 3].at(...[-1]));                                    // 3
console.log([1, 2, 3].join(...[","]));                                 // 1,2,3
const spl = [9, 8, 7, 6, 5];
spl.splice(...[1, 2]);
console.log(spl.join(","));                                            // 9,6,5

// --- variadic methods with dedicated spread arms must still work ---------
const pushed: number[] = [1, 2];
pushed.push(...[3, 4, 5]);
console.log(pushed.join(","));                                         // 1,2,3,4,5
console.log([1, 2].concat(...[[3, 4], [5]]).join(","));                // 1,2,3,4,5
const uns: number[] = [3, 4];
uns.unshift(...[1, 2]);
console.log(uns.join(","));                                            // 1,2,3,4

// --- general `...variable` spread (not just an inline literal) -----------
const cb = [(x: number) => x * 100] as const;
console.log([1, 2].map(...cb).join(","));                                // 100,200

// --- arbitrary-expression receiver ---------------------------------------
console.log(Array.from([1, 2, 3]).filter(...[(x: number) => x !== 2]).join(",")); // 1,3

// --- non-spread regression guard (fast path must still fire) -------------
console.log([1, 2, 3].map((x: number) => x - 1).join(","));             // 0,1,2

// --- contrast: forms that already worked, kept as guards -----------------
console.log(Math.max(...[4, 7, 2]));                                    // 7
const id = (x: number) => x;
console.log(id(...[42]));                                               // 42

// --- queueMicrotask spread: must fire the callback (prints last) ---------
queueMicrotask(...[() => console.log("microtask-ran")]);                // microtask-ran
const qcb = [() => console.log("microtask-var")] as const;
queueMicrotask(...qcb);                                                 // microtask-var
