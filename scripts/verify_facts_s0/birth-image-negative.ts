// S0 `birthimage` (birth cost): a complete constructor-free birth writes
// every slot of its image in the nursery arm, with no shared fill. Under
// PERRY_FACTS_SABOTAGE=birthimage the image drops its last store; the
// check-mode witness must report the slot left unwritten.
const keep: unknown[] = [];
let s = 0;
for (let i = 0; i < 20; i++) {
  const r = { a: i, b: i + 0.5, c: "c" + i, d: i * 2, e: i % 2 === 0, f: [i] };
  keep.push(r);
  s += r.a + r.d + r.f[0];
}
console.log(s, keep.length);
