// Wide keys: `xs[(n >>> 0) - 3]` reads with a key the facts bound to
// [-3, 4294967292]: an integer past int32 that may be negative, so it is no
// wide key and the read keeps its normalizer (-1 reads the named property,
// undefined here). Under PERRY_FACTS_SABOTAGE=widekey such a key is claimed
// non-negative: the read would convert -1 as an unsigned index. The
// check-mode witness at the site must report it ("wide key").
function pick(xs: number[], n: number): number {
  return xs[(n >>> 0) - 3] ?? 7;
}
const xs: number[] = [4, 5, 6];
let s = 0;
for (let i = 2; i < 6; i++) s += pick(xs, process.argv.length > 100 ? 9 : i);
console.log(s);
