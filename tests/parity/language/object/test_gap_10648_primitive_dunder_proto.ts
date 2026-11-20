// #10648: Boolean and BigInt primitives inherit the Object.prototype
// __proto__ getter just as Number and String primitives do.
const truth: any = true;
const falsity: any = false;
const positive: any = 1n;
const negative: any = -1n;

console.log("true proto", truth.__proto__ === Boolean.prototype);
console.log("false proto", falsity.__proto__ === Boolean.prototype);
console.log("positive bigint proto", positive.__proto__ === BigInt.prototype);
console.log("negative bigint proto", negative.__proto__ === BigInt.prototype);
console.log("number control", (5 as any).__proto__ === Number.prototype);
console.log("string control", ("s" as any).__proto__ === String.prototype);
