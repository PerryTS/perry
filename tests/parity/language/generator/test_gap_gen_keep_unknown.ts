// An iterable taken from unknown input: generic path, same output.
function* g(n: number): Generator<number> { for (let i = 0; i < n; i++) yield i; }
const input: any = JSON.parse(process.argv[2] ?? "[1,2,3]");
const src: any = Array.isArray(input) && input.length > 5 ? input : g(input.length);
let s = 0;
for (const v of src) s += v;
console.log("unknown", s);
