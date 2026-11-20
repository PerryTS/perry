// repro 1: bare accumulator past 2^31
let big = 2147483640;
for (let k = 0; k < 10; k++) big++;
console.log("big:", big);              // node: 2147483650

let dn = -2147483640;
for (let k = 0; k < 10; k++) dn--;
console.log("dn:", dn);                // node: -2147483650

// a normal small counter must still be correct (and can stay fast)
let c = 0;
for (let k = 0; k < 1000; k++) c++;
console.log("c:", c);                  // 1000

// index-used counter still works (should keep i32 fast path)
const arr = new Array(5).fill(0);
let s = 0;
for (let i = 0; i < 5; i++) { arr[i] = i * 2; s += arr[i]; }
console.log("arr sum:", s, "arr:", arr.join(","));
