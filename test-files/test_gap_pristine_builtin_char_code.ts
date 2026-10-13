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
(String.prototype as any).charCodeAt = function (this: string, i: number) { return 1000 + i; };
console.log(codes('abc'), anyCodes('abc'), anyCodes('xyzwvutsrq'));
