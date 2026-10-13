// #11915: a call reads its method BEFORE evaluating its arguments, so a patch
// made while they are evaluated does not change the method that call runs,
// and an argument's valueOf runs exactly once.
const sp: any = String.prototype;
const origSlice = sp.slice;
function patchSlice(): number {
  sp.slice = function () { return 'patched'; };
  return 1;
}
console.log('abcd'.slice(patchSlice(), 3));
console.log('abcd'.slice(1, 3));
sp.slice = origSlice;
const log: string[] = [];
const idx = { valueOf() { log.push('v'); return 1; } };
const s = 'abcd';
console.log(s.slice(idx as any, 3), s.indexOf('c', idx as any), log.join(''));
const any1: any = 'xyz';
console.log(any1.charCodeAt(idx), any1.at(-1), log.join(''));
