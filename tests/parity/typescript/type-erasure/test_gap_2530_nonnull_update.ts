// #2530 — update expression (`++`/`--`) on a non-null-asserted member.
// `obj.x!++` is valid TS: the `!` (TSNonNullExpression) is a compile-time-only
// assertion and must be transparent to update-expression lowering. Perry used
// to reject it with U006 ("Update expression only supports identifiers and
// member expressions") because the wrapper wasn't unwrapped before the
// identifier/member check. Parenthesized operands `(obj.x)++` are the same case.
// Output is byte-for-byte vs `node --experimental-strip-types`.

const counts: Record<string, number> = { roles: 0 };
counts.roles!++;
console.log(counts.roles); // 1

let obj = { count: 5 };
obj.count!--;
console.log(obj.count); // 4

// Prefix form on a non-null-asserted member.
++obj.count!;
console.log(obj.count); // 5

// Parenthesized operand.
(obj.count)++;
console.log(obj.count); // 6

// Computed member with non-null assertion.
const arr: number[] = [10];
arr[0]!++;
console.log(arr[0]); // 11

// Plain identifier with non-null assertion still works.
let n: number = 41;
n!++;
console.log(n); // 42
