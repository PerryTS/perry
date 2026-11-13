// Loops without calls over Strings whose reads and operators run no
// program code (quiet sites, `PERRY_FACTS_QUIET_STRINGS`): a String's own
// `length`, and the code unit (or, past the end, `undefined`) an element
// read of a String gives. Each loop keeps its length in a slot (the legacy
// length hoist): `PERRY_ARRAY_BASE_DIAG=1` prints one `[perry quiet] ...
// verdict=kept` line per loop here; with `PERRY_FACTS_QUIET_STRINGS=0` the
// loops that use a code unit or a String's `length` as an operand print
// `refused:site` (the walker of #14537 without the String rules).
function reverse(s: string): string {
  let r = "";
  for (let m = 0; m < s.length; m++) {
    r = s[m] + r;
  }
  return r;
}
function pastTheEnd(s: string): string {
  let r = "";
  for (let m = 0; m < s.length; m++) {
    r = r + s[m + 2] + "|";
  }
  return r;
}
function totalLength(parts: string[]): number {
  let t = 0;
  for (let i = 0; i < parts.length; i++) {
    t += parts[i].length;
  }
  return t;
}
function widths(parts: string[], out: number[]): number[] {
  for (let i = 0; i < out.length; i++) {
    out[i] = parts[i % parts.length].length * 10 + out[i];
  }
  return out;
}
function interleave(a: string, b: string): string {
  let r = "";
  for (let i = 0; i < a.length; i++) {
    r += a[i] + b[i % b.length];
  }
  return r;
}
function surrogates(s: string): number {
  let n = 0;
  for (let i = 0; i < s.length; i++) {
    n += (s[i] + "").length;
  }
  return n;
}
const words = "alpha beta gamma delta".split(" ");
console.log(reverse("hello"), reverse(""), reverse("a b"));
console.log(pastTheEnd("abcd"));
console.log(totalLength(words), totalLength([]), totalLength(["é", "日本", "\u{1F600}"]));
console.log(widths(words, [1, 2, 3, 4, 5, 6]).join(","));
console.log(interleave("abc", "xy"), interleave("", "z"));
console.log(surrogates("x\u{1F600}y"));
