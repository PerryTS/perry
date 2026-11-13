// Negative control of `test_array_quiet_strings`: `String.prototype` gets an
// index getter, so a code unit read past the end of a primitive String goes
// up to it and runs program code, which pops the Array the loop walks. The
// program writes a native prototype: no element site of it is quiet
// (`chain:native-write`), and the loop reads its length at every step
// (`PERRY_ARRAY_BASE_DIAG=1`: `refused`, never `kept`), as in node.
// `String.prototype` gets an index getter: a code unit read past the end of
// a primitive String goes up to it.
function protoIndex(t: string, h: { xs: number[] }): string {
  const xs = h.xs;
  let s = "";
  for (let i = 0; i < xs.length; i++) {
    s += xs[i] + t[i + 2] + ",";
  }
  return s;
}
const h3 = { xs: [1, 2, 3, 4, 5, 6] };
Object.defineProperty(String.prototype, "5", {
  get() {
    h3.xs.pop();
    return "p";
  },
  configurable: true,
});
console.log(protoIndex("abc", h3));
