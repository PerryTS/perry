function countUnits(s: string, code: number): number {
  let count: number = 0;
  for (let i: number = 0; i < s.length; i++) {
    if (s.charCodeAt(i) === code) count++;
  }
  return count;
}
function weighted(s: string, code: number): number {
  let sum: number = 0;
  for (let i: number = 0; i < s.length; i++) {
    const value = s.charCodeAt(i);
    if (value < code) sum += value;
    else sum -= value;
  }
  return sum;
}
for (const s of ["", "a;b;", "long;ascii;payload;", "界;😀;", "\ud800;\udc00", "é;é", "\u0000;\u007f"]) {
  console.log(countUnits(s, 59), countUnits(s, 0xd83d), weighted(s, 100));
}
let calls: number = 0;
const fake: any = { get length() { calls++; return 4; }, charCodeAt(i: number) { calls++; return i === 2 ? 59 : 65; } };
console.log(countUnits(fake as string, 59), calls);
const previous = String.prototype.charCodeAt;
String.prototype.charCodeAt = function (i: number): number { return i % 2 === 0 ? 59 : 65; };
console.log(countUnits("long ASCII input with patched method", 59));
String.prototype.charCodeAt = previous;
console.log(countUnits("after;restore;", 59));
function numericMutation(s: string): number {
  let x: number = 2;
  for (let i: number = 0; i < s.length; i++) {
    if (s.charCodeAt(i) > 50) { x += i; x *= 1.01; }
    else { x -= i; }
  }
  return x;
}
console.log(numericMutation("a;bc"));
function numericInput(s: string, initial: any): any {
  let n: any = initial;
  for (let i: number = 0; i < s.length; i++) {
    if (s.charCodeAt(i) === 59) n += 1;
    else n -= 1;
  }
  return n;
}
console.log(numericInput("a;b;", 7), numericInput("a;b;", "prefix"));
function shifted(s: string): number {
  let n: number = 0;
  for (let i: number = 0; i < s.length; i++) n += s.charCodeAt(i + 1);
  return n;
}
console.log(shifted("abc"), shifted("界😀"));
function mixed(s: string, out: Int32Array): number {
  let n: number = 0;
  for (let i: number = 0; i < s.length; i++) {
    const code = s.charCodeAt(i);
    if (code >= 59) { out[i] = code; n += code; }
    else { out[i] = -code; n -= code; }
  }
  return n;
}
const out = new Int32Array(8);
console.log(mixed("a;z:!", out), out.join(","));
