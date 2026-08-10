// Gap: a spread method call whose RECEIVER is an imported binding (#7191).
//
// `arr.map(...[cb])` where `arr` comes from another module was wrong four ways
// out of four — and two of them were silently wrong rather than throwing, which
// is why it survived: `arr.slice(...[1,3])` returned the whole array and
// `nums.includes(...[20])` returned false, because the spread argument list
// arrived as a single array sitting in the builtin's first slot.
//
// The cause is that `Expr::ExternFuncRef` is not one shape. It is both an
// imported FUNCTION — where a method-apply dispatch would be wrong, which is
// why the generic `CallSpread` tail skips it — and an imported VALUE, where
// method-apply is exactly what is needed. The skip did not distinguish them, so
// a spread call on any imported receiver never dispatched the receiver's
// method. `ctx.imported_vars` already records which imports are variables
// rather than functions.
//
// `test_gap_6718_spread_call_native_method` covers inline literals, local
// receivers and `queueMicrotask` — not imported receivers — so this shape had
// never been under test. Both halves are asserted here: the imported-value
// receivers that were broken, and the imported-function/class/object receivers
// the skip exists to protect, which must keep working.

import { arr, nums, fn, obj, K } from "./spread_call_imported_receiver_7191_helper.ts";

const cb = (x: number) => x * 2;

// The four cases from the report, in their original order.
console.log(JSON.stringify(arr.map(...([cb] as [any]))));
console.log(JSON.stringify(arr.slice(...([1, 3] as [any, any]))));
console.log(JSON.stringify(nums.join(...(["-"] as [any]))));
console.log(JSON.stringify(nums.includes(...([20] as [any]))));

// More imported-array receivers, including one whose spread arg is itself an
// array (so "the args became one array" cannot pass by accident).
console.log("indexOf:", arr.indexOf(...([2] as [any])));
console.log("concat:", JSON.stringify(arr.concat(...([[9]] as [any]))));
console.log("at:", nums.at(...([-1] as [any])));

// The shapes the skip protects: an imported FUNCTION reached through
// `.call` / `.apply`, and a direct spread call on it.
console.log("fn.call:", fn.call(...([null, 1, 2] as [any, any, any])));
console.log("fn.apply:", fn.apply(...([null, [3, 4]] as [any, any])));
console.log("fn direct:", fn(...([5, 6] as [any, any])));

// An imported object's method, and an imported class's static and instance
// methods — all reached with a spread argument list.
console.log("obj.m:", obj.m(...([2] as [any])));
console.log("K.s:", K.s(...([1] as [any])));
console.log("K#m:", new K().m(...([10] as [any])));

// A local receiver must be unaffected by any of this.
const local = [3, 1, 2];
console.log("local-map:", JSON.stringify(local.map(...([cb] as [any]))));
console.log("local-slice:", JSON.stringify(local.slice(...([1, 3] as [any, any]))));
