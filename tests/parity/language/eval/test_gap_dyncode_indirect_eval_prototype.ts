// DEC 250 locks Array.prototype; the eval'd source patches a user array
// prototype UAP that the array adopts, a write the compiler cannot see either.
const UAP: any = Object.create(Array.prototype);
(globalThis as any).UAP = UAP;
const a: number[] = Object.setPrototypeOf([1, 2], UAP);
const evaluate = eval;
const source = process.argv[2] || "UAP.join = function () { return 'indirect-patch'; }";
evaluate(source);
console.log(a.join(','));
