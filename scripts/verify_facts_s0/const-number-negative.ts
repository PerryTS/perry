// `K` is 16 at run time, and 8 on a path the program never takes: its
// interval is [8, 16]. Under PERRY_FACTS_SABOTAGE=constnum the facts export
// a bounded module const as its lower bound, so every read of `K` is claimed
// to be 8: the read's witness must report it.
function pick(n: number): number {
  return n > 1000 ? 1 << 3 : 1 << 4;
}
const K = pick(process.argv.length);
function scale(v: number): number {
  return v * K + (v & K);
}
console.log(scale(3) + scale(16));
