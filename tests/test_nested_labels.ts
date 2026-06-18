// #5247 regression: a label chain ending in a loop (`outer: inner: for`) must
// treat BOTH labels as loop labels, so `continue outer` / `break outer` target
// the real loop — not a synthetic run-once do-while. Output must match
// `node --experimental-strip-types`.

// continue the outer label of a chain (CodeRabbit's example).
let hits = 0;
outer: inner: for (let i = 0; i < 3; i++) {
    hits++;
    if (i < 2) continue outer;
}
console.log(hits); // 3

// continue the outer label from inside a truly-inner loop.
const cont: number[] = [];
a: b: for (let i = 0; i < 3; i++) {
    for (let j = 0; j < 3; j++) {
        cont.push(i * 10 + j);
        if (j === 1) continue a;
    }
}
console.log(cont.join(",")); // 0,1,10,11,20,21

// break the outer label from inside a truly-inner loop.
const brk: number[] = [];
p: q: for (let i = 0; i < 3; i++) {
    for (let j = 0; j < 3; j++) {
        brk.push(i * 10 + j);
        if (i === 1 && j === 1) break p;
    }
}
console.log(brk.join(",")); // 0,1,2,10,11

// the innermost label of a chain still works on its own.
let k = 0;
x: y: for (let i = 0; i < 4; i++) {
    k++;
    if (i % 2 === 0) continue y;
}
console.log(k); // 4
