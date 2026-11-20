// Gap test for #5347 — Object.assign / object-spread with an ARRAY source.
// An array source's indexed elements live in the ArrayHeader element buffer,
// not in an ObjectHeader keys_array; reading that field off an array used to
// deref garbage and SIGSEGV. Also covers the boxed-String target read-only
// negative (assigning an in-range index to a primitive-string target throws).
// Compared byte-for-byte against `node --experimental-strip-types`.

// ---- array source: indexed elements are copied (was a crash) ----
console.log(JSON.stringify(Object.assign({}, [1, 2])));          // {"0":1,"1":2}
console.log(JSON.stringify({ ...[10, 20, 30] }));                // {"0":10,"1":20,"2":30}
console.log(JSON.stringify(Object.assign({ x: 1 }, [7, 8])));    // {"x":1,"0":7,"1":8}
console.log(JSON.stringify(Object.assign({}, { a: 1 }, [9], { b: 2 }))); // {"a":1,"0":9,"b":2}

// ---- array source with a named expando: indices THEN expando, in order ----
const arr: any = [1, 2];
arr.foo = "z";
console.log(JSON.stringify(Object.assign({}, arr)));             // {"0":1,"1":2,"foo":"z"}

// ---- array target still grows from an array source ----
console.log(JSON.stringify(Object.assign([0, 0, 0], [9, 9])));   // [9,9,0]

// ---- boxed-String target: an in-range index write is read-only -> TypeError ----
function thrown(fn: () => void): string {
  try { fn(); return "no throw"; } catch (e: any) { return e.constructor.name; }
}
console.log(thrown(() => Object.assign("a", [1])));              // TypeError
console.log(thrown(() => Object.assign("abc", { 1: "x" })));    // TypeError
console.log(thrown(() => Object.assign("abc", { 5: "x" })));    // no throw (out of range)
console.log(thrown(() => Object.assign("", { 0: "x" })));       // no throw (empty string)

// ---- normal object merges still must not throw ----
console.log(JSON.stringify(Object.assign({ a: 1 }, { b: 2 }))); // {"a":1,"b":2}
