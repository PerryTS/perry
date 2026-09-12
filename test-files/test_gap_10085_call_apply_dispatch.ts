// #10085: Function.prototype.call/apply and direct spread calls all reach
// js_closure_call_array. Keep its cached non-rest path and its rest-bundling
// detour semantically identical across every supported dispatch arity.

import { ImportedClass } from "./fixtures/issue_10085_dispatch/imported.ts";

function check(condition: boolean, message: string): void {
  if (!condition) throw new Error(message);
}

function restShape(first: number, ...rest: number[]): string {
  return `${first}|${rest.length}|${rest.length === 0 ? -1 : rest[rest.length - 1]}`;
}

const restFunctions: any[] = [];
restFunctions.push(restShape);
const restValue: any = restFunctions[0];

// Rest closures must still bundle through .call, .apply, and direct spread at
// arities 1..=16. This pins #653's high-arity fix while the metadata probe is
// switched from a registry lookup to the dispatch-strategy memo.
for (let n = 1; n <= 16; n++) {
  const args: number[] = [];
  for (let i = 1; i <= n; i++) args.push(i);
  const expected = `1|${n - 1}|${n === 1 ? -1 : n}`;
  const direct = restValue(...args);
  const viaCall = restValue.call({ ignored: true }, ...args);
  const viaApply = restValue.apply({ ignored: true }, args);
  check(direct === expected, `rest direct arity ${n}: ${direct}`);
  check(viaCall === expected, `rest call arity ${n}: ${viaCall}`);
  check(viaApply === expected, `rest apply arity ${n}: ${viaApply}`);
}

// A 16-parameter non-rest closure called with 0..=16 values exercises every
// js_closure_callN arm. Missing values must still be padded with undefined.
function arity16(
  a0: any,
  a1: any,
  a2: any,
  a3: any,
  a4: any,
  a5: any,
  a6: any,
  a7: any,
  a8: any,
  a9: any,
  a10: any,
  a11: any,
  a12: any,
  a13: any,
  a14: any,
  a15: any,
): number {
  const values = [
    a0,
    a1,
    a2,
    a3,
    a4,
    a5,
    a6,
    a7,
    a8,
    a9,
    a10,
    a11,
    a12,
    a13,
    a14,
    a15,
  ];
  let count = 0;
  for (const value of values) if (value !== undefined) count++;
  return count;
}

for (let n = 0; n <= 16; n++) {
  const args: number[] = [];
  for (let i = 0; i < n; i++) args.push(i + 1);
  const direct = (arity16 as any)(...args);
  const viaCall = (arity16 as any).call(null, ...args);
  const viaApply = (arity16 as any).apply(null, args);
  check(direct === n, `arity16 direct ${n}: ${direct}`);
  check(viaCall === n, `arity16 call ${n}: ${viaCall}`);
  check(viaApply === n, `arity16 apply ${n}: ${viaApply}`);
}

function ordinary(this: { bias: number }, left: number, right: number): number {
  return this.bias + left + right;
}

const receiver = {
  bias: 40,
  ordinary,
  makeArrow() {
    return (value: number) => this.bias + value;
  },
};
check(receiver.ordinary(1, 1) === 42, "ordinary direct receiver");
check(receiver.ordinary.call(receiver, 1, 1) === 42, "ordinary call receiver");
check(receiver.ordinary.apply(receiver, [1, 1]) === 42, "ordinary apply receiver");

const arrow = receiver.makeArrow();
check(arrow(2) === 42, "arrow direct receiver");
check(arrow.call({ bias: 100 }, 2) === 42, "arrow call keeps lexical receiver");
check(arrow.apply({ bias: 100 }, [2]) === 42, "arrow apply keeps lexical receiver");

const bound = ordinary.bind(receiver, 1);
check(bound(1) === 42, "bound direct receiver");
check(bound.call({ bias: 100 }, 1) === 42, "bound call keeps receiver");
check(bound.apply({ bias: 100 }, [1]) === 42, "bound apply keeps receiver");

function invokeImportedClass(value: any, input: number): string {
  return `${typeof value}:${value.describe(input)}`;
}

// Imported class refs share the 0x7FFE tag with bridged int32 values. They
// must remain callable objects while call-array unboxes genuine integers.
const classArgs: any[] = [ImportedClass, 7];
const classExpected = "function:imported:7";
check((invokeImportedClass as any)(...classArgs) === classExpected, "class direct spread");
check(
  (invokeImportedClass as any).call(null, ...classArgs) === classExpected,
  "class call spread",
);
check(
  (invokeImportedClass as any).apply(null, classArgs) === classExpected,
  "class apply",
);

console.log("ok");
