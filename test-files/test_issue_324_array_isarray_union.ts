// Regression: Array.isArray on a value typed as a union containing an
// array variant must dispatch to the runtime, not constant-fold to true.
// Previously the codegen fast path used `is_array_expr` which treats any
// Union-with-Array-variant as array-typed (correct for routing
// .length/.push dispatch on `T[] | null` after truthy narrowing, wrong
// for Array.isArray). Result: every call site picked the array branch
// regardless of the runtime tag.

class C {
  hook(value: number | readonly number[]) {
    if (Array.isArray(value)) {
      const required: number[] = [];
      for (const item of value) required.push(item);
      if (required.length === 0) {
        throw new Error("Hook must have at least one required component");
      }
      console.log("array branch", required.length);
    } else {
      console.log("single branch", value);
    }
  }
}

console.log("Array.isArray(1)", Array.isArray(1));
console.log("Array.isArray([1,2])", Array.isArray([1, 2]));

try {
  new C().hook(1);
  console.log("hook(1) OK");
} catch (e) {
  console.log("hook(1) ERROR", e instanceof Error ? e.message : e);
}

try {
  new C().hook([10, 20, 30]);
  console.log("hook([10,20,30]) OK");
} catch (e) {
  console.log("hook([...]) ERROR", e instanceof Error ? e.message : e);
}

try {
  new C().hook([]);
  console.log("hook([]) OK");
} catch (e) {
  console.log("hook([]) ERROR", e instanceof Error ? e.message : e);
}

// Also exercise the union-with-undefined and union-with-string shapes
// that go through the same Union-static-type path.
function check(x: string | number[]): string {
  return Array.isArray(x) ? "arr:" + x.length : "str:" + x;
}
console.log("check('hi')", check("hi"));
console.log("check([1,2,3])", check([1, 2, 3]));

function checkOpt(x: number[] | undefined): string {
  if (Array.isArray(x)) return "arr:" + x.length;
  return "no-arr";
}
console.log("checkOpt(undefined)", checkOpt(undefined));
console.log("checkOpt([7,8])", checkOpt([7, 8]));

// And confirm the fast-path TRUE still fires on a definitively-Array type.
function takeArray(xs: number[]): boolean {
  return Array.isArray(xs);
}
console.log("takeArray([1])", takeArray([1]));
