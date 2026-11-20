// Erased annotations do not establish the number-only helper contract.
const boxed: any = { valueOf() { console.log("valueOf"); return 2; } };
function claimedNumber(): number { return boxed; }
console.log("call", "abcdef".charAt(claimedNumber()));
const saved: number = claimedNumber();
console.log("local", "abcdef".slice(saved, 4));
const holder: {pos: number} = {pos: boxed};
console.log("field", "abcdef".charAt(holder.pos));
console.log("search", "abcabc".indexOf("c", claimedNumber()));
console.log("end", "abcdef".substring(1, claimedNumber()));
