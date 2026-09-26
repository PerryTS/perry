// Bare-loop control: pure JS work, no I/O, no event loop.
const N = Number(process.argv[2] || 1000);
let acc = 0;
for (let i = 0; i < N * 1000; i++) acc = (acc + i * 7) % 1000003;
console.log("loop", N, acc);
