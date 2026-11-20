// #6300: a parenthesized / TS-cast assignment target must NOT bypass the
// `const` immutability check. `(c as any) = 9` used to silently mutate the
// binding with no diagnostic — only the bare-identifier store path checked
// immutability, and every wrapper spelling (parens, `as`, `satisfies`, `!`,
// `<T>`) routed around it into a second, unchecked identifier-store arm.
//
// Perry appends the binding name to the message (`... variable 'c'.`) where
// V8 does not, so normalize that away to stay byte-for-byte identical to
// `node --experimental-strip-types`.
function norm(e: unknown): string {
  const err = e as Error;
  const isTypeError = err instanceof TypeError;
  const msg = err.message.replace(/ '[^']*'\.$/, ".");
  return `${isTypeError ? "TypeError" : "Error"}: ${msg}`;
}

function expectThrow(label: string, fn: () => void): void {
  try {
    fn();
    console.log(`${label}: NO THROW`);
  } catch (e) {
    console.log(`${label}: ${norm(e)}`);
  }
}

// --- const targets: every wrapper must throw and leave the binding intact ---

const c1 = 1;
expectThrow("bare", () => {
  // @ts-expect-error assignment to const
  c1 = 9;
});
console.log("c1 =", c1);

const c2 = 2;
expectThrow("paren", () => {
  // @ts-expect-error assignment to const
  (c2) = 9;
});
console.log("c2 =", c2);

const c3 = 3;
expectThrow("as-cast", () => {
  (c3 as any) = 9;
});
console.log("c3 =", c3);

const c4 = 4;
expectThrow("satisfies", () => {
  (c4 satisfies number) = 9;
});
console.log("c4 =", c4);

const c5 = 5;
expectThrow("non-null", () => {
  // @ts-expect-error assignment to const
  (c5!) = 9;
});
console.log("c5 =", c5);

const c6 = 6;
expectThrow("nested-wrappers", () => {
  ((c6 as any)!) = 9;
});
console.log("c6 =", c6);

// --- compound / update / logical assignment through a cast ---

const c7 = 7;
expectThrow("compound-add", () => {
  (c7 as any) += 1;
});
console.log("c7 =", c7);

const c8 = 8;
expectThrow("compound-shift", () => {
  (c8 as any) <<= 1;
});
console.log("c8 =", c8);

const c9 = 9;
expectThrow("postfix-incr", () => {
  (c9 as any)++;
});
console.log("c9 =", c9);

const c10 = 10;
expectThrow("prefix-decr", () => {
  --(c10 as any);
});
console.log("c10 =", c10);

const c11 = 11;
expectThrow("bare-incr", () => {
  // @ts-expect-error update of const
  c11++;
});
console.log("c11 =", c11);

// A BigInt const passes ToNumeric cleanly, so `++` still reaches PutValue and
// reports the const TypeError.
const c12 = 12n;
expectThrow("bigint-incr", () => {
  (c12 as any)++;
});
console.log("c12 =", c12);

const c13: number | null = null;
expectThrow("nullish-assign", () => {
  (c13 as any) ??= 13;
});
console.log("c13 =", c13);

const c14 = 0;
expectThrow("or-assign", () => {
  (c14 as any) ||= 14;
});
console.log("c14 =", c14);

// The RHS is evaluated before PutValue rejects the binding, so its side
// effects are observable even though the assignment throws.
const c15 = 15;
let rhsRuns = 0;
expectThrow("rhs-side-effect", () => {
  (c15 as any) = (rhsRuns++, 99);
});
console.log("c15 =", c15, "rhsRuns =", rhsRuns);

// A short-circuiting logical assignment never reaches PutValue, so a truthy
// const with `||=` must NOT throw.
const c16 = 16;
expectThrow("or-assign-shortcircuit", () => {
  (c16 as any) ||= 99;
});
console.log("c16 =", c16);

// A const inside a function body behaves the same.
function inFunction(): void {
  const local = 1;
  expectThrow("fn-local", () => {
    (local as any) = 9;
  });
  console.log("fn-local =", local);
}
inFunction();

// `for (const x of ...)` binds a fresh immutable binding per iteration.
for (const item of [1]) {
  expectThrow("for-of-const", () => {
    (item as any) = 9;
  });
  console.log("item =", item);
}

// --- legitimate writes must keep working ---

let d1 = 1;
(d1 as any) = 9;
console.log("let as-cast:", d1);

let d2 = 1;
(d2) = 9;
console.log("let paren:", d2);

let d3 = 1;
(d3 as any) += 4;
console.log("let compound:", d3);

let d4 = 1;
(d4 as any)++;
++(d4 as any);
console.log("let update:", d4);

let d5: number | null = null;
(d5 as any) ??= 7;
console.log("let nullish:", d5);

let d6 = 1;
(d6 satisfies number) = 6;
(d6!) = d6 + 1;
console.log("let satisfies/non-null:", d6);

// A cast MEMBER expression target is a property write, not a binding write —
// it stays legal even when the object itself is `const`.
const obj: any = { x: 0 };
(obj as any).x = 1;
console.log("obj.x =", obj.x);
(obj as any).x++;
console.log("obj.x =", obj.x);
(obj as any)["x"] += 10;
console.log("obj.x =", obj.x);

// Mutating a `const` object/array is fine — only rebinding is not.
const arr: number[] = [1];
arr.push(2);
(arr as any)[0] = 5;
console.log("arr =", arr.join(","));
