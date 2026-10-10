function exercise(a: any, k: any) {
let sum = 0
for (let i = 0; i < 1000000; i++) { sum += a.has(k) ? 1 : 0 }
console.log(sum)
}
const receivers: any[] = [new WeakMap(), {}]; const a: any = receivers[0]; const k = {}; a.set(k, 7)
exercise(a, k)
