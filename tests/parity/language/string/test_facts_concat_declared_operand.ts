// A `+` lowered as the facts' string concatenation (`expr/facts_string.rs`)
// whose operand is a string only by a declaration: `t[i]` on `t: string` is
// declared a string and is `undefined` past the end, so `xs[i] + t[i]` is a
// Number there (NaN). The lowering is right either way (the concat helper
// dispatches on the tags); check mode (`PERRY_VERIFY_FACTS=1`) must not take
// the declaration for a fact and report a false proof (it did: rc 86,
// "string operand @0", observed NaN). Negative controls: the same reads with
// a String.prototype index getter that pops the walked Array (the miss reads
// the chain), a `?:` of a declared read and a call, a string[] with holes
// read through a call, and a `+` whose string side is a call the facts prove
// a string (that one stays witnessed: `label` is declared `string | number`,
// so only the facts call it a string).

function name(n: number): string {
  return "n" + n;
}

function label(n: number): string | number {
  return "l" + n;
}

function protoIndex(t: string, h: { xs: number[] }): string {
  const xs = h.xs;
  let s = "";
  for (let i = 0; i < xs.length; i++) {
    s += xs[i] + t[i + 2] + ",";
  }
  return s;
}

function declaredBin(t: string, xs: number[]): string {
  let s = "";
  for (let i = 0; i < xs.length; i++) s += xs[i] + t[i] + ",";
  return s;
}

function declaredCond(t: string, k: number): string {
  return (k > 1 ? t[k] : name(k)) + name(k);
}

function inlinedRead(t: string, k: number) {
  return t[k];
}

function arrRead(a: string[], k: number) {
  return a[k];
}

function factsBin(xs: number[]): string {
  let s = "";
  for (let i = 0; i < xs.length; i++) s += xs[i] + label(i) + label(i + 1);
  return s;
}

console.log(declaredBin("ab", [1, 2, 3, 4]));
console.log(declaredCond("abc", 0) + declaredCond("abc", 2) + declaredCond("abc", 9));
console.log(factsBin([1, 2, 3]));
const holes: string[] = ["a", "b", "c"];
holes[6] = "z";
let holed = "";
for (let k = 0; k < 8; k++) holed += arrRead(holes, k) + arrRead(holes, k + 1) + ";";
console.log(holed);
let before = "";
for (let k = 0; k < 7; k++) before += inlinedRead("abc", k) + inlinedRead("xy", k) + ";";
console.log(before);

const h = { xs: [1, 2, 3, 4, 5, 6] };
Object.defineProperty(String.prototype, "5", {
  get() {
    h.xs.pop();
    return "p";
  },
  configurable: true,
});
console.log(protoIndex("abc", h));
let after = "";
for (let k = 0; k < 7; k++) after += inlinedRead("abc", k) + inlinedRead("xy", k) + ";";
console.log(after);
