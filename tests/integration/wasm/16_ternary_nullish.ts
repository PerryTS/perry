// Ternary, nullish coalescing, optional chaining patterns
const a = true ? "yes" : "no";
console.log(a);

const b = false ? "yes" : "no";
console.log(b);

const c: string | undefined = undefined;
const d = c ?? "default";
console.log(d);

const e = "value" ?? "default";
console.log(e);

// typeof
console.log(typeof 42);
console.log(typeof "hello");
console.log(typeof true);
