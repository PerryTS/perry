// #10727: an array local captured by a nested closure read back `undefined` in
// its OWN declaring scope, once `Array.prototype` had ever carried an indexed
// property. The closure kept seeing the correct array, so the two storages for
// one binding disagreed and `S() === dest` was false.
//
// Introduced by #10488's `ctx.local_types.insert` on the redeclaration branch
// of `lower_let`. `Stmt::PreallocateBoxes` pre-creates the box for a captured
// local before its `Stmt::Let`, so the `Let` lands on that same redeclaration
// branch and recorded the refined type (`Array(Any)`) for a local whose real
// storage is a box. Predicates reading `local_types` then lowered reads as raw
// local loads instead of `js_box_get_bits`.
//
// The trigger is rare (something must put an indexed property on
// `Array.prototype`) but the affected shape — an array local captured by a
// closure — is ordinary code.

function show(v: unknown): string {
  if (v === undefined) return "undefined";
  if (v === null) return "null";
  if (typeof v === "object") return "object";
  return typeof v + "(" + String(v) + ")";
}

// Arm the array-prototype index deopt, then remove the property again. The
// latch that this sets is monotone: deleting the property does not clear it.
Object.defineProperty(Array.prototype, "11", {
  configurable: true,
  get() {
    return "proto11";
  },
});
delete (Array.prototype as Record<string, unknown>)["11"];

// 1. the regression: a captured array local, read from its declaring scope
function capturedArray(): void {
  const dest: unknown[] = new Array(12);
  dest[0] = { tag: "zero" };
  for (let i = 1; i < 12; i++) {
    dest[i] = i;
  }
  function peek(): unknown[] {
    return dest;
  }
  console.log("captured typeof:", typeof dest);
  console.log("captured isArray:", Array.isArray(dest));
  console.log("captured length:", (dest as unknown[]).length);
  console.log("captured [0]:", show(dest[0]));
  console.log("captured [5]:", show(dest[5]));
  console.log("captured [11]:", show(dest[11]));
  console.log("captured identity:", peek() === dest);
  console.log("captured via closure:", peek().length, show(peek()[5]));
}

// 2. control: the same array with no closure over it
function plainArray(): void {
  const dest: unknown[] = new Array(12);
  dest[0] = { tag: "zero" };
  for (let i = 1; i < 12; i++) {
    dest[i] = i;
  }
  console.log("plain length:", dest.length, "[5]:", show(dest[5]));
}

// 3. control: a captured NON-array local
function capturedObject(): void {
  const held = { a: 1, b: 2 };
  function peek(): { a: number; b: number } {
    return held;
  }
  console.log("object a:", held.a, "identity:", peek() === held);
}

// 4. a captured array that is mutated after the closure is created
function capturedThenMutated(): void {
  const rows: number[] = [];
  function collect(value: number): void {
    rows.push(value);
  }
  collect(1);
  collect(2);
  rows.push(3);
  console.log("mutated length:", rows.length, "join:", rows.join(","));
}

// 5. the #10488 shape must keep working: a hoisted `var` redeclared in both
// branches, then compared against an out-of-bounds read.
function hoistedVarRedeclare(flag: boolean): void {
  if (flag) {
    var nums = [1, 2, 3];
  } else {
    var nums = [4, 5];
  }
  console.log("var redeclare:", nums.length, nums[10] === undefined, nums[0]);
}

capturedArray();
plainArray();
capturedObject();
capturedThenMutated();
hoistedVarRedeclare(true);
hoistedVarRedeclare(false);
