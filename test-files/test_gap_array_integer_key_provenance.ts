// Integer-producing keys, rejected Number keys, and evaluation order.
const table: any[] = [10, 20, 30, 40];
(table as any)["-1"] = 71;
(table as any)["NaN"] = 72;
(table as any)["1.5"] = 73;
(table as any)["Infinity"] = 74;
(table as any)["2147483648"] = 75;
function read(k: number) { return table[k]; }
function write(k: number, v: number) { table[k] = v; return table[k]; }
for (const k of [0, -0, 1.5, -1, NaN, Infinity, 2147483648]) {
  console.log(read(k), write(k, 90), read(k));
}
function chars(s: string) {
  for (let i = 0; i <= s.length; i++) {
    const c = s.charCodeAt(i);
    console.log(table[c], table[c & 3], table[c | 0]);
    table[c] = i + 100;
    console.log(table[c]);
  }
}
chars("\x00\x01\x03");
function native(k: number) { console.log(table[k | 0], table[k & 3], table[k >>> 0]); }
native(-1); native(4294967296); native(1.5);
let count = 0;
function key() { count++; return 1.5; }
function value() { count += 10; return count; }
table[key()] = value();
console.log(count, table[1.5], table[1]);
// A reassignment revokes the char-code fact.
let c = "a".charCodeAt(0);
c = 1.5;
console.log(table[c]);
// A var redeclaration can reuse a parameter ID without a LocalSet.
function rebound(flag: boolean, c: number) {
  if (flag) { var c = "\x01".charCodeAt(0); }
  return table[c];
}
console.log(rebound(false, 1.5), rebound(true, 1.5));
function redeclared(flag: boolean) {
  var c = 1.5;
  if (flag) { var c = "\x01".charCodeAt(0); }
  return table[c];
}
console.log(redeclared(false), redeclared(true));

// A later fractional writer also invalidates earlier reads on a loop backedge.
function loopWriter() {
  let c = "\x01".charCodeAt(0);
  for (let i = 0; i < 2; i++) { console.log(table[c]); c = 1.5; }
}
loopWriter();
function largeWriter() {
  let c = "\x01".charCodeAt(0);
  for (let i = 0; i < 2; i++) { console.log(table[c]); c = 2147483648; }
}
largeWriter();
function surrogateWriter(s: string) {
  for (let i = 0; i < s.length; i++) {
    let c = s.charCodeAt(i);
    console.log(table[c], table[c & 3]);
    if (c >= 0xd800 && c < 0xdc00) {
      c = 0x10000 + (((c & 0x3ff) << 10) | (s.charCodeAt(++i) & 0x3ff));
      console.log(table[c], table[c & 3]);
    }
  }
}
surrogateWriter("x\ud83d\ude00");
function hoisted(flag: boolean) {
  if (flag) { var c = "\x01".charCodeAt(0); }
  console.log(table[c]);
}
hoisted(false); hoisted(true);
function lying(s: string) {
  const c = s.charCodeAt(0);
  return table[c];
}
console.log(lying({ charCodeAt() { return 1.5; } } as any));
// Prototype lookup can produce a fractional key even on a real string.
(String.prototype as any).charCodeAt = function () { return 1.5; };
function patched() {
  const s = String("abc");
  const c = s.charCodeAt(0);
  return table[c];
}
console.log(patched());
