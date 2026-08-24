// #8694: stable monomorphic registry enumeration must lower through the
// guarded, allocation-free helper rather than materializing generic key lists
// at every call.  Keep this intentionally close to perform-ecs' one-key
// ComponentGroupRegistry hot path.
const groups: any = {};
groups[3] = 1;

function sumRegistry(): number {
  let total = 0;
  for (const groupHash in groups) total += groups[groupHash];
  return total;
}

let checksum = 0;
for (let i = 0; i < 200_000; i++) checksum += sumRegistry();
console.log(`for_in_stable_keys:${checksum}`);
