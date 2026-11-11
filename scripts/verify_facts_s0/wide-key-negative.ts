// Wide keys: `xs[z * 1000000000 - 1]` reads with a key the facts bound to
// [-1, 4999999999]: an integer past int32 that may be negative, so it is no
// wide key and the read keeps its normalizer (-1 reads the named property,
// undefined here). Under PERRY_FACTS_SABOTAGE=widekey such a key is claimed
// non-negative: the read would convert -1 as an unsigned index. The
// check-mode witness at the site must report it ("wide key").
function pick(xs: number[], z: number): number {
  return xs[z * 1000000000 - 1] ?? 7;
}
const xs: number[] = [4, 5, 6];
const z = process.argv.length > 100 ? 5 : 0;
let s = 0;
for (let i = 0; i < 3; i++) s += pick(xs, z);
console.log(s);
