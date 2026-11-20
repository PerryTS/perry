// ECMA-262 13.3.6.1 ArgumentListEvaluation: every argument expression runs, left to right.
const k = Number(process.argv[2] ?? "0");
let n = 0;
const side = (): any => { n++; return 9; };
console.log(Math.cos(k, side()), n);
console.log(Math.sin(k, side()), n);
console.log(Math.sqrt(k + 4, side()), n);
console.log(Math.floor(k + 1.5, side()), n);
console.log(Math.abs(k - 3, side()), n);
console.log(Math.pow(k + 2, 3, side()), n);
console.log(Math.atan2(k + 1, 1, side()), n);
console.log(Math.imul(k + 3, 5, side()), n);
console.log(typeof Math.random(side()), n);
console.log(Math.max(k, 1, side()), n);
console.log(Math.min(k, 1, side()), n);
console.log(Math.hypot(k + 3, 4, side()), n);
