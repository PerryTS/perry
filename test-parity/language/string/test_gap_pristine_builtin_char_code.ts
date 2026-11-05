// #11915: the inline charCodeAt reads the method its receiver reads, on a
// proven string and on a tag-guarded one, before and after a replacement.
function codes(s: string): number {
  let t = 0;
  for (let i = 0; i < s.length; i++) t += s.charCodeAt(i);
  return t;
}
function anyCodes(s: any): number {
  let t = 0;
  for (let i = 0; i < s.length; i++) t += s.charCodeAt(i);
  return t;
}
console.log(codes('abc'), anyCodes('abc'), anyCodes('xyzwvutsrq'));
// DEC 250 (intrinsic-only): a primitive string's methods come only from
// String.prototype. The replace is refused now (expected file); the reads
// after it still run.
try { (String.prototype as any).charCodeAt = function (this: string, i: number) { return 1000 + i; }; } catch (e) { console.log("DEC 250: " + (e as Error).message); }
console.log(codes('abc'), anyCodes('abc'), anyCodes('xyzwvutsrq'));
