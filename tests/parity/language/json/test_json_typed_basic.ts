// Issue #179 typed-parse plan, Step 1 tests: verifies
// `JSON.parse<T>(blob)` runtime behavior matches `JSON.parse(blob) as T`
// byte-for-byte against Node. The `<T>` is TypeScript-erased and has
// NO runtime effect on correctness — Perry may use it for faster
// codegen but must return identical values.

interface Item {
  id: number;
  name: string;
  active: boolean;
}

const blob = '{"id":42,"name":"alpha","active":true}';

const typed = JSON.parse<Item>(blob);
console.log("typed.id:" + typed.id);
console.log("typed.name:" + typed.name);
console.log("typed.active:" + typed.active);

// Must be equivalent to the untyped call
const untyped = JSON.parse(blob);
console.log("untyped.id:" + untyped.id);
console.log("untyped.name:" + untyped.name);
console.log("untyped.active:" + untyped.active);
