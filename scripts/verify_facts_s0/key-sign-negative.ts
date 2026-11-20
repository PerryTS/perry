// Key signs: `xs[this.k]` reads with a key the facts bound to [-2, 1]: an
// int32, so the read takes no normalizer, but it may be negative, so it
// keeps its sign test and -2 reads the named property (undefined here).
// Under PERRY_FACTS_SABOTAGE=keysign every int32 key is claimed
// non-negative: the read would enter the element tier with -2. The
// check-mode witness at the site must report it ("key sign").
class Holder {
  k: number = process.argv.length > 100 ? 1 : -2;
  xs: number[] = [4, 5, 6];
  get(): number {
    return this.xs[this.k] ?? 7;
  }
}
const h = new Holder();
let s = 0;
for (let i = 0; i < 3; i++) s += h.get();
console.log(s);
