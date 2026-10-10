const a = new WeakMap<object, number>(); const k = {}; a.set(k, 7)
let sum = 0
for (let i = 0; i < 1000000; i++) { sum += a.get(k)! }
console.log(sum)
