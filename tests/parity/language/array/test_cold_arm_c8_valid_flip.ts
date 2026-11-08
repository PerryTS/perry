// b2 C8: the loop starts on an array with an element accessor (the fast form
// is off); a slow-arm getter replaces it with a plain numeric array, and the
// later reads are exact on the new one.
let a: number[] = [1, 2, 3, 4];
Object.defineProperty(a, 2, { get() { a = [10, 20, 30, 40]; return 33; }, configurable: true });
const out: number[] = [];
for (let r = 0; r < 3; r++) for (let k = 0; k < 4; k++) out.push(a[(k * 5) & 3]);
console.log(out.join(" "));
