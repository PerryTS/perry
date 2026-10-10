// Body reference: same brand-checking intrinsic in an own method slot.
function exercise(a: any, k: any) {
let sum = 0
for (let i = 0; i < 1000000; i++) { sum += a.get(k) }
console.log(sum)
}
const receivers: any[] = [new WeakMap(), {}]; const a: any = receivers[0]; const k = {}; a.set(k, 7)
a.get = WeakMap.prototype.get;
exercise(a, k)
